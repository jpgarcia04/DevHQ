//! Reading an installation: the installed executable's file that an `installation` reference
//! declares on a workstation. It answers «which version is installed, and since when?» for a
//! system no HTTP check can ask: one that runs only on demand, or a desktop application without a
//! server. The declared version is text somebody typed; what the file says is observed.
//!
//! Observing never gives the source a way to run code or change anything. The declared path, and
//! the file there, are untrusted data, whoever declared them. So DevHQ never runs the executable
//! (no `--version`), never loads it as an image, and never asks Windows' version APIs, which load
//! it: it reads the file's bytes, with a reader of its own for the version resource (`pe`), in a
//! process of its own per file (`devhq --read-installation`, see `reader`) that first forbids
//! itself to start programs (`local_copies::confine`), within a deadline it meets by killing that
//! process, and it treats what comes back as untrusted data. A crash or a hang of the reader on a
//! hostile file ends the reader, never the window or the gate.
//!
//! A reading says what the file was when it was read: whether it was there, where the system
//! really kept it, its SHA-256, size and modification time, what its version resource declares,
//! and how many processes were running from it. To know that last fact it looks at the system's
//! list of processes, and keeps of it only two numbers: no process identifier, no command line, no
//! other program's path.
//!
//! What is not guaranteed: code that a hostile file managed to run inside the reader would have
//! the person's rights; nothing of the system stops the reader writing, only its code, which
//! opens the file to read and nothing else, and the hostile battery (`battery`) do.

#[cfg(test)]
mod battery;
mod pe;
mod processes;
mod reader;
pub(crate) mod readings;

use crate::inventory::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long DevHQ waits for one installation: its file is hashed whole, however large. Past it,
/// nothing is kept.
pub(crate) const DEADLINE: Duration = Duration::from_secs(30);
/// Everything a reader may answer for one installation: one short line.
const MAX_ANSWER: u64 = 64 * 1024;
/// The longest reason kept: why a file was not read, or why its version or its processes are
/// unknown.
pub(crate) const MAX_DETAIL: usize = 1000;
/// The longest path kept as where the system really keeps a file.
pub(crate) const MAX_STORED_AT: usize = 4096;

/// What a reading can find at a declared path.
pub(crate) const OUTCOMES: [&str; 4] = ["read", "missing", "unreadable", "not_an_executable"];

/// The reader, as each reading names it: which readings a reader made, if one turns out to have a
/// fault. The number changes with what the reader reads or how.
pub(crate) const READER: &str = "DevHQ installation reader 2";

/// An installation read by a reader. Only `outcome`, `detail` and, when the system keeps the file
/// elsewhere, `storedAt`, when the file was not read; `detail` is then why.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Facts {
    pub outcome: String,
    pub detail: Option<String>,
    /// Where the system really keeps the file, when that is not the path asked for.
    pub stored_at: Option<String>,
    /// SHA-256 of the file's bytes, in lowercase hexadecimal, and how many they were.
    pub sha256: Option<String>,
    pub size_bytes: Option<u64>,
    /// Milliseconds since the epoch, as the file system says.
    pub modified_at: Option<i64>,
    /// What the file's version resource declares, as the file says it…
    pub product_version: Option<String>,
    pub file_version: Option<String>,
    pub product_name: Option<String>,
    /// …or why that is unknown.
    pub version_unknown: Option<String>,
    /// The processes running from the file, besides the reader itself, and the processes named
    /// like the file that the system did not let the reader look at…
    pub processes: Option<u32>,
    pub processes_unexamined: Option<u32>,
    /// …or why they are unknown.
    pub processes_unknown: Option<String>,
}

/// `devhq --read-installation`: reads the file stdin names and answers on stdout. Returns the
/// process exit code.
pub fn read_installation_main() -> i32 {
    reader::serve(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        reader::Confinement::Required,
    )
}

/// How DevHQ starts a reader: this executable with `--read-installation`. Tests start theirs.
#[derive(Debug, Clone)]
pub(crate) struct Reader {
    program: PathBuf,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
}

impl Reader {
    pub(crate) fn this_executable() -> Result<Self, ApiError> {
        let program = std::env::current_exe().map_err(|error| {
            ApiError::read(format!("DevHQ could not find its own executable: {error}"))
        })?;
        Ok(Self {
            program,
            args: vec!["--read-installation".into()],
            envs: Vec::new(),
        })
    }

    /// Reads the file at `path` in a process of its own, waiting at most `deadline`. An error is
    /// a failure of DevHQ, which is kept nowhere.
    pub(crate) fn read(&self, path: &str, deadline: Duration) -> Result<Facts, ApiError> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.envs.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // A development build is a console program: without this, each reader would open a
            // console window.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|error| {
            ApiError::read(format!("DevHQ could not start its reader: {error}"))
        })?;
        let request = serde_json::json!({ "path": path }).to_string();
        if let Some(mut stdin) = child.stdin.take() {
            // A reader that already ended (it could not confine itself) closes stdin; its answer
            // says why.
            let _ = stdin.write_all(request.as_bytes());
        }
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ApiError::read("DevHQ could not read its reader's answer."));
        };
        // Lines arrive on a thread of their own, so that waiting has a deadline.
        let (lines, received) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout.take(MAX_ANSWER)).lines() {
                if lines.send(line).is_err() {
                    break;
                }
            }
        });
        let started = Instant::now();
        let answer = loop {
            let remaining = deadline.saturating_sub(started.elapsed());
            match received.recv_timeout(remaining) {
                // Only JSON objects are the reader's answer. Nothing else writes to its stdout but
                // a test harness running a reader, whose own lines are skipped.
                Ok(Ok(line)) if !line.trim_start().starts_with('{') => {}
                Ok(Ok(line)) => break Ok(Some(line)),
                Ok(Err(error)) => {
                    break Err(format!("DevHQ could not read its reader's answer: {error}"))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    break Err(format!(
                    "Reading {path} took longer than {} s, so DevHQ stopped it. Nothing was kept.",
                    deadline.as_secs()
                ))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(None),
            }
        };
        // Whatever it was doing, the reader is done: killing an ended process does nothing.
        let _ = child.kill();
        let status = child.wait().ok();
        let line = answer.map_err(ApiError::read)?.ok_or_else(|| {
            ApiError::read(format!(
                "The reader stopped without an answer ({}). Nothing was kept.",
                status.map_or_else(
                    || "its exit is unknown".to_owned(),
                    |status| status.to_string()
                )
            ))
        })?;
        answered(&line).and_then(validated).map_err(ApiError::read)
    }
}

/// The one line a reader answers: what it read, or why DevHQ could not read.
fn answered(line: &str) -> Result<Facts, String> {
    let malformed = |error: &dyn std::fmt::Display| {
        format!("The reader answered something DevHQ cannot accept: {error}")
    };
    let mut value: Value = serde_json::from_str(line).map_err(|error| malformed(&error))?;
    let phase = value
        .as_object_mut()
        .and_then(|object| object.remove("phase"))
        .and_then(|phase| phase.as_str().map(str::to_owned))
        .ok_or_else(|| malformed(&"a line without its phase"))?;
    match phase.as_str() {
        "installation" => serde_json::from_value(value).map_err(|error| malformed(&error)),
        "error" => {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| malformed(&"an error without its message"))?;
            Err(format!(
                "DevHQ could not read this installation: {}",
                plain_text(message, MAX_DETAIL)
            ))
        }
        other => Err(malformed(&format!("an unknown phase {other}"))),
    }
}

/// Text as DevHQ keeps it from a file it does not trust, or from the system about it: control
/// characters become spaces and it is cut to `max` characters.
pub(crate) fn plain_text(text: &str, max: usize) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().count() <= max {
        cleaned.to_owned()
    } else {
        let mut cut: String = cleaned.chars().take(max - 1).collect();
        cut.push('…');
        cut
    }
}

/// A text a reader returned: present, within `max` characters, without control characters and
/// without surrounding whitespace.
fn checked_text(text: &str, max: usize, what: &str) -> Result<(), String> {
    if text.is_empty()
        || text.chars().count() > max
        || text.chars().any(char::is_control)
        || text.trim() != text
    {
        return Err(format!(
            "The reader returned a {what} that DevHQ cannot accept."
        ));
    }
    Ok(())
}

/// What a reader returned, if it holds together: the reader is DevHQ's own code, but it read a
/// hostile file, so what it says is checked as untrusted, against the same rules the schema's
/// CHECKs keep.
fn validated(facts: Facts) -> Result<Facts, String> {
    if !OUTCOMES.contains(&facts.outcome.as_str()) {
        return Err("The reader returned an unknown outcome.".into());
    }
    if let Some(stored_at) = &facts.stored_at {
        checked_text(
            stored_at,
            MAX_STORED_AT,
            "place where the system keeps the file",
        )?;
        if facts.outcome == "missing" {
            return Err("The reader said where the system keeps a file that is not there.".into());
        }
    }
    if facts.outcome != "read" {
        let only_why = Facts {
            outcome: facts.outcome.clone(),
            detail: facts.detail.clone(),
            stored_at: facts.stored_at.clone(),
            ..Facts::default()
        };
        let Some(detail) = &facts.detail else {
            return Err("The reader did not say why it could not read the file.".into());
        };
        checked_text(detail, MAX_DETAIL, "reason")?;
        if facts != only_why {
            return Err("The reader returned facts of a file it did not read.".into());
        }
        return Ok(facts);
    }
    if facts.detail.is_some() {
        return Err("The reader returned a reason for a file it read.".into());
    }
    let (Some(sha256), Some(size)) = (&facts.sha256, facts.size_bytes) else {
        return Err("The reader left out the hash or the size of a file it read.".into());
    };
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err("The reader returned a hash that is no SHA-256.".into());
    }
    if i64::try_from(size).is_err() {
        return Err("The reader returned a size no file has.".into());
    }
    if let Some(modified_at) = facts.modified_at {
        if crate::records::format_time(modified_at).is_none() {
            return Err(
                "The reader returned a modification time that is no moment DevHQ can show.".into(),
            );
        }
    }
    let declared = [
        (&facts.product_version, "ProductVersion"),
        (&facts.file_version, "FileVersion"),
        (&facts.product_name, "ProductName"),
    ];
    for (text, what) in declared {
        if let Some(text) = text {
            checked_text(text, pe::MAX_TEXT, what)?;
        }
    }
    let any_declared = declared.iter().any(|(text, _)| text.is_some());
    match &facts.version_unknown {
        Some(_) if any_declared => {
            return Err("The reader returned a version and a reason why it is unknown.".into())
        }
        Some(reason) => checked_text(reason, MAX_DETAIL, "reason")?,
        None if !any_declared => {
            return Err("The reader returned neither a version nor why it is unknown.".into())
        }
        None => {}
    }
    match (
        facts.processes,
        facts.processes_unexamined,
        &facts.processes_unknown,
    ) {
        (Some(_), Some(_), None) => {}
        (None, None, Some(reason)) => checked_text(reason, MAX_DETAIL, "reason")?,
        _ => {
            return Err(
                "The reader returned processes that are neither counted nor unknown.".into(),
            )
        }
    }
    Ok(facts)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::path::Path;

    /// The environment variable that makes `serves_as_the_reader` a reader, and how.
    const SERVE: &str = "DEVHQ_TEST_INSTALLATION_READER";

    /// A complete answer, as a reader gives it for a file it read.
    const READ: &str = r#"{"phase":"installation","outcome":"read","detail":null,"storedAt":null,"sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","sizeBytes":0,"modifiedAt":1000,"productVersion":"1.4.0","fileVersion":null,"productName":null,"versionUnknown":null,"processes":0,"processesUnexamined":0,"processesUnknown":null}"#;

    /// Run by the tests in a process of its own, as DevHQ runs `devhq --read-installation`; on its
    /// own, it does nothing. `confined` serves as the real reader does; `unconfined` reads without
    /// forbidding programs; anything else misbehaves as it says.
    #[test]
    #[ignore = "a helper that other tests run in a process of their own"]
    fn serves_as_the_reader() {
        let Some(mode) = std::env::var_os(SERVE) else {
            return;
        };
        let (stdin, mut stdout) = (std::io::stdin().lock(), std::io::stdout().lock());
        let code = match mode.to_str() {
            Some("confined") => reader::serve(stdin, stdout, reader::Confinement::Required),
            Some("unconfined") => reader::serve(stdin, stdout, reader::Confinement::Skipped),
            // Nothing at all: stuck on a file.
            Some("silent") => {
                std::thread::sleep(Duration::from_secs(60));
                0
            }
            // Gone without a word.
            Some("gone") => 3,
            Some("flood") => {
                let flood = "x".repeat(2 * 1024 * 1024);
                writeln!(
                    stdout,
                    r#"{{"phase":"installation","outcome":"missing","detail":"{flood}"}}"#
                )
                .unwrap();
                0
            }
            // Any other mode is an edit of a good answer: `old=>new`.
            Some(edit) => {
                let (old, new) = edit.split_once("=>").unwrap();
                assert!(READ.contains(old), "{old}");
                writeln!(stdout, "{}", READ.replace(old, new)).unwrap();
                0
            }
            None => 2,
        };
        // Before the harness prints anything more on stdout.
        std::process::exit(code);
    }

    /// This test executable, running only `serves_as_the_reader` as `mode`.
    pub(crate) fn test_reader(mode: &str) -> Reader {
        Reader {
            program: std::env::current_exe().unwrap(),
            args: [
                "--ignored",
                "--exact",
                "installations::tests::serves_as_the_reader",
                "--nocapture",
                "-q",
            ]
            .map(OsString::from)
            .to_vec(),
            envs: vec![(SERVE.into(), mode.into())],
        }
    }

    /// A folder of its own in the temporary folder, as the file system names it.
    pub(crate) fn folder(name: &str) -> PathBuf {
        let folder = crate::inventory::tests::disposable(
            crate::inventory::tests::resolved_temp_dir()
                .join(format!("devhq-{name}-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// A synthetic executable, without code, whose version resource declares `version` as its
    /// ProductVersion: what tests elsewhere install where they need an installation.
    pub(crate) fn executable(version: &str) -> Vec<u8> {
        pe::tests::image(
            &pe::tests::version_data(&[("ProductVersion", version), ("ProductName", "Shop")]),
            true,
        )
        .0
    }

    pub(crate) fn path_text(path: &Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    pub(crate) fn sha256_of(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn meets_its_deadline_and_refuses_what_it_cannot_accept() {
        let quick = Duration::from_secs(3);
        let good = test_reader("phase=>phase")
            .read("/anywhere", quick)
            .unwrap();
        assert_eq!(good.product_version.as_deref(), Some("1.4.0"));
        let started = Instant::now();
        for (how, message) in [
            (
                "silent",
                "took longer than 3 s, so DevHQ stopped it. Nothing was kept.",
            ),
            ("gone", "The reader stopped without an answer"),
            ("flood", "cannot accept"),
            // What a reader broken by a hostile file could say, each refused whole.
            (r#""outcome":"read"=>"outcome":"opened""#, "unknown outcome"),
            (r#""installation"=>"something""#, "unknown phase"),
            (
                r#""detail":null=>"detail":"Why""#,
                "a reason for a file it read",
            ),
            (
                r#""sizeBytes":0=>"sizeBytes":null"#,
                "left out the hash or the size",
            ),
            (r#"e3b0c442=>E3B0C442"#, "no SHA-256"),
            (
                r#""sizeBytes":0=>"sizeBytes":18446744073709551615"#,
                "a size no file has",
            ),
            (
                r#""modifiedAt":1000=>"modifiedAt":9223372036854775807"#,
                "no moment DevHQ can show",
            ),
            (
                r#""productVersion":"1.4.0"=>"productVersion":"1.4.0\n""#,
                "ProductVersion",
            ),
            (
                r#""productVersion":"1.4.0"=>"productVersion":"""#,
                "ProductVersion",
            ),
            (
                r#""productVersion":"1.4.0"=>"productVersion":null"#,
                "neither a version nor why",
            ),
            (
                r#""versionUnknown":null=>"versionUnknown":"Unknown.""#,
                "a version and a reason",
            ),
            (
                r#""processes":0=>"processes":null"#,
                "neither counted nor unknown",
            ),
            (
                r#""processesUnknown":null=>"processesUnknown":"Unknown.""#,
                "neither counted nor unknown",
            ),
            (
                r#""storedAt":null=>"storedAt":"""#,
                "where the system keeps the file",
            ),
            (
                r#""processes":0=>"pids":[4],"processes":0"#,
                "cannot accept",
            ),
            (
                r#""outcome":"read","detail":null=>"outcome":"missing","detail":"Nothing.""#,
                "facts of a file it did not read",
            ),
        ] {
            let error = test_reader(how).read("/anywhere", quick).unwrap_err();
            assert_eq!(error.code, "read_error");
            assert!(error.message.contains(message), "{how}: {}", error.message);
        }
        assert!(
            started.elapsed() < Duration::from_secs(40),
            "it waited for the reader"
        );
    }
}
