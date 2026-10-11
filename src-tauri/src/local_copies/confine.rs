//! Keeps the reader's own process from starting programs, before it reads anything.
//!
//! libgit2's read APIs have no way to start a process, and the hostile battery holds them to it.
//! This is the system's own barrier on top: if libgit2, or a path through it the battery missed,
//! ever tried to start a program, the system would refuse. It applies to the reader's process
//! only. It does not contain code that hostile data managed to run inside the reader: that code
//! could still ask another process (WMI or the Task Scheduler on Windows, systemd or D-Bus on
//! Linux) to start one, and could write wherever the person can. Nothing here stops writes.
//!
//! - Windows: the process puts itself in a Job Object that allows one active process, itself, so
//!   creating any other fails (ERROR_NOT_ENOUGH_QUOTA, 1816). Jobs nest, so this works inside the
//!   job of a packaged app or of a CI runner.
//! - Linux: a seccomp filter answers EPERM to `execve` and `execveat`, so no program can replace
//!   the reader or start in a child of it (children inherit the filter). It applies to root too,
//!   unlike `RLIMIT_NPROC`, which does not stop `execve` at all. On x86_64 it also refuses the x32
//!   system calls, whose `execve` has another number.
//! - Anywhere else: the reader cannot be kept from starting programs, so it does not read.

/// Forbids this process to start programs from now on, for as long as it lives. An error says
/// why it could not; the reader then reads nothing.
pub(crate) fn forbid_programs() -> Result<(), String> {
    imp::forbid_programs()
}

#[cfg(windows)]
mod imp {
    use std::mem::{size_of, zeroed};
    use std::ptr::null;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    pub(super) fn forbid_programs() -> Result<(), String> {
        // SAFETY: plain Win32 calls on a zeroed structure the API defines and on handles they
        // return. The job's handle is never closed: the job lives as long as the process, and
        // closing it would end the process (KILL_ON_JOB_CLOSE).
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return Err(format!(
                    "Windows could not create a job for the reader: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            limits.BasicLimitInformation.ActiveProcessLimit = 1;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(format!(
                    "Windows could not limit the reader's job to one process: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
                return Err(format!(
                    "Windows could not put the reader in its job: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(())
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod imp {
    use libc::{
        sock_filter, sock_fprog, SYS_execve, SYS_execveat, BPF_ABS, BPF_JEQ, BPF_JGE, BPF_JMP,
        BPF_K, BPF_LD, BPF_RET, BPF_W, EPERM, PR_SET_NO_NEW_PRIVS, PR_SET_SECCOMP,
        SECCOMP_MODE_FILTER, SECCOMP_RET_ALLOW, SECCOMP_RET_ERRNO, SECCOMP_RET_KILL_PROCESS,
    };

    // The architecture the kernel reports for this build's system calls (linux/audit.h), which
    // libc does not define.
    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 0xC000_00B7;

    // Where `struct seccomp_data` keeps the system call number and the architecture.
    const NR: u32 = 0;
    const ARCH: u32 = 4;

    fn statement(code: u32, k: u32) -> sock_filter {
        sock_filter {
            code: code as u16,
            jt: 0,
            jf: 0,
            k,
        }
    }

    fn jump(code: u32, k: u32, jt: u8, jf: u8) -> sock_filter {
        sock_filter {
            code: code as u16,
            jt,
            jf,
            k,
        }
    }

    pub(super) fn forbid_programs() -> Result<(), String> {
        let refuse = SECCOMP_RET_ERRNO | EPERM as u32;
        let mut filter = vec![
            // A system call made for another architecture is not this program's: end it.
            statement(BPF_LD | BPF_W | BPF_ABS, ARCH),
            jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH, 1, 0),
            statement(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
            statement(BPF_LD | BPF_W | BPF_ABS, NR),
        ];
        if cfg!(target_arch = "x86_64") {
            // The x32 calls share the architecture and set this bit in the number.
            filter.push(jump(BPF_JMP | BPF_JGE | BPF_K, 0x4000_0000, 0, 1));
            filter.push(statement(BPF_RET | BPF_K, refuse));
        }
        for call in [SYS_execve, SYS_execveat] {
            filter.push(jump(BPF_JMP | BPF_JEQ | BPF_K, call as u32, 0, 1));
            filter.push(statement(BPF_RET | BPF_K, refuse));
        }
        filter.push(statement(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));
        let program = sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };
        // prctl reads its arguments as unsigned longs; passing them as such leaves no high bits to
        // chance in the variadic call.
        let (none, one): (libc::c_ulong, libc::c_ulong) = (0, 1);
        // SAFETY: prctl with the arguments the kernel documents; `program` and the filter it
        // points to outlive the call, and the kernel copies them.
        unsafe {
            // Required to install a filter without privileges, and it keeps a program from gaining
            // any through set-user-ID, which no longer matters once none can start.
            if libc::prctl(PR_SET_NO_NEW_PRIVS, one, none, none, none) != 0 {
                return Err(format!(
                    "Linux refused no_new_privs for the reader: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if libc::prctl(
                PR_SET_SECCOMP,
                libc::c_ulong::from(SECCOMP_MODE_FILTER),
                &raw const program as libc::c_ulong,
                none,
                none,
            ) != 0
            {
                return Err(format!(
                    "Linux refused the reader's seccomp filter: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        Ok(())
    }
}

#[cfg(not(any(
    windows,
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
)))]
mod imp {
    pub(super) fn forbid_programs() -> Result<(), String> {
        Err("DevHQ cannot keep a reader from starting programs on this system, so it does not read local copies here.".into())
    }
}
