//! Reading a local copy: a working copy of a system's repository that a `local_copy` reference
//! declares on a workstation. It answers «what has had no activity for months?» and «what work
//! exists only on one machine?», and keeps HEAD's commit for comparing with what is deployed.
//!
//! Observing never gives the source a way to run code or change anything. The declared path, and
//! the repository there, are untrusted data, whoever declared them: a repository can name programs
//! in its config, attributes and hooks, and the `git` program runs them (filter drivers, fsmonitor,
//! signing programs…). So DevHQ never runs `git`. It reads with libgit2's read APIs, which start no
//! program and write nothing, in a process of its own per copy (`devhq --read-copy`, see `reader`)
//! that first forbids itself to start programs where the system allows it (see `confine`), within a
//! deadline it meets by killing that process, and it treats what comes back as untrusted data.
//! A crash or a hang of libgit2 on hostile data ends the reader, never the window or the gate.
//!
//! What is not guaranteed: code that hostile data managed to run inside the reader would have the
//! person's rights (it could write files, or ask another process to start a program); and nothing
//! of the system stops the reader writing, only libgit2 and the hostile battery do.

#[cfg(test)]
mod battery;
mod confine;
mod reader;
pub(crate) mod readings;
pub(crate) mod remotes;

use crate::inventory::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Unpublished commits are counted up to here; one more reads as «more than this».
pub(crate) const UNPUBLISHED_LIMIT: u32 = 1000;
/// How long DevHQ waits for one copy. A reading whose refs arrived keeps them, with the working
/// tree unknown; without them, nothing is kept.
pub(crate) const DEADLINE: Duration = Duration::from_secs(30);
/// Everything a reader may answer for one copy. A line of the protocol is far shorter.
const MAX_ANSWER: u64 = 1024 * 1024;

/// What a reading can find at a declared path.
pub(crate) const OUTCOMES: [&str; 4] = ["read", "missing", "not_a_copy", "unreadable"];
/// An operation left halfway, as `repository.state()` names it.
pub(crate) const OPERATIONS: [&str; 6] = [
    "merge",
    "rebase",
    "cherry_pick",
    "revert",
    "bisect",
    "apply_mailbox",
];

/// A remote of a copy: its name, and its URL when it is a form DevHQ keeps (see `remotes`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Remote {
    pub name: String,
    pub url: Option<String>,
}

/// Everything a reading says but the working tree. Only `outcome` and `detail` when the copy was
/// not read; `detail` is then why.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Refs {
    pub outcome: String,
    pub detail: Option<String>,
    pub branch: Option<String>,
    pub head: Option<String>,
    /// Milliseconds since the epoch, as the commit itself says.
    pub last_commit_at: Option<i64>,
    pub unpublished_on_head: Option<u32>,
    pub unpublished_elsewhere: Option<u32>,
    pub stashes: Option<u32>,
    pub synced_at: Option<i64>,
    pub remotes: Option<Vec<Remote>>,
    pub operation: Option<String>,
}

/// The working tree: its three counts, or why it is unknown.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Worktree {
    pub changed: Option<u32>,
    pub untracked: Option<u32>,
    pub unverifiable: Option<u32>,
    pub unknown: Option<String>,
}

impl Worktree {
    pub(crate) fn unknown(reason: &str) -> Self {
        Self {
            unknown: Some(reader::plain_text(reason, reader::MAX_DETAIL)),
            ..Self::default()
        }
    }
}

/// A copy read by a reader, checked: what DevHQ will keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    pub refs: Refs,
    /// None when the copy was not read (no working tree to speak of).
    pub worktree: Option<Worktree>,
}

/// The reader, as its version names it: which readings a reader made, if one turns out to have a
/// fault.
pub(crate) fn reader_name() -> String {
    let (major, minor, patch) = git2::Version::get().libgit2_version();
    format!("libgit2 {major}.{minor}.{patch}")
}

/// `devhq --read-copy`: reads the copy stdin names and answers on stdout. Returns the process
/// exit code.
pub fn read_copy_main() -> i32 {
    reader::serve(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        reader::Confinement::Required,
    )
}

/// How DevHQ starts a reader: this executable with `--read-copy`. Tests start theirs.
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
            args: vec!["--read-copy".into()],
            envs: Vec::new(),
        })
    }

    /// Reads the copy at `path` in a process of its own, waiting at most `deadline`.
    pub(crate) fn read(&self, path: &str, deadline: Duration) -> Result<Facts, ApiError> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // libgit2 needs the person's environment to find their Git config, and Git for Windows'
        // (PATH, the profile folders); a GIT_* variable could redirect what it reads.
        for (key, _) in std::env::vars_os() {
            if key
                .to_str()
                .is_some_and(|key| key.to_ascii_uppercase().starts_with("GIT_"))
            {
                command.env_remove(key);
            }
        }
        command.envs(self.envs.iter().map(|(key, value)| (key, value)));
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
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ApiError::read("DevHQ could not read its reader's answer."))?;
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
        let mut refs: Option<Refs> = None;
        let mut worktree: Option<Worktree> = None;
        let mut failure: Option<String> = None;
        let mut timed_out = false;
        loop {
            let remaining = deadline.saturating_sub(started.elapsed());
            let line = match received.recv_timeout(remaining) {
                Ok(Ok(line)) => line,
                Ok(Err(error)) => {
                    failure = Some(format!("DevHQ could not read its reader's answer: {error}"));
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    timed_out = true;
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            // Only JSON objects are the reader's answer. Nothing else writes to its stdout but a
            // test harness running a reader, whose own lines are skipped.
            if !line.trim_start().starts_with('{') {
                continue;
            }
            match answer_line(&line) {
                Ok(Line::Refs(read)) if refs.is_none() => {
                    let done = read.outcome != "read";
                    refs = Some(read);
                    if done {
                        break;
                    }
                }
                Ok(Line::Worktree(read)) if refs.is_some() && worktree.is_none() => {
                    worktree = Some(read);
                    break;
                }
                Ok(Line::Error(message)) => {
                    failure = Some(message);
                    break;
                }
                Ok(_) => {
                    failure = Some("The reader answered out of order.".into());
                    break;
                }
                Err(message) => {
                    failure = Some(message);
                    break;
                }
            }
        }
        // Whatever it was doing, the reader is done: killing an ended process does nothing.
        let _ = child.kill();
        let status = child.wait().ok();
        if let Some(message) = failure {
            return Err(ApiError::read(message));
        }
        let Some(refs) = refs else {
            return Err(ApiError::read(if timed_out {
                format!(
                    "Reading {path} took longer than {} s, so DevHQ stopped it. Nothing was kept.",
                    deadline.as_secs()
                )
            } else {
                format!(
                    "The reader stopped without an answer ({}). Nothing was kept.",
                    status.map_or_else(
                        || "its exit is unknown".to_owned(),
                        |status| status.to_string()
                    )
                )
            }));
        };
        let refs = validated_refs(refs).map_err(ApiError::read)?;
        if refs.outcome != "read" {
            return Ok(Facts {
                refs,
                worktree: None,
            });
        }
        let worktree = match worktree {
            Some(worktree) => validated_worktree(worktree).map_err(ApiError::read)?,
            None if timed_out => Worktree::unknown(&format!(
                "Reading the working tree took longer than {} s, so DevHQ stopped it.",
                deadline.as_secs()
            )),
            None => Worktree::unknown("The reader stopped before it read the working tree."),
        };
        Ok(Facts {
            refs,
            worktree: Some(worktree),
        })
    }
}

/// One line of a reader's answer.
enum Line {
    Refs(Refs),
    Worktree(Worktree),
    Error(String),
}

fn answer_line(line: &str) -> Result<Line, String> {
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
        "refs" => serde_json::from_value(value)
            .map(Line::Refs)
            .map_err(|error| malformed(&error)),
        "worktree" => serde_json::from_value(value)
            .map(Line::Worktree)
            .map_err(|error| malformed(&error)),
        "error" => {
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| malformed(&"an error without its message"))?;
            Ok(Line::Error(format!(
                "DevHQ could not read this copy: {}",
                reader::plain_text(message, reader::MAX_DETAIL)
            )))
        }
        other => Err(malformed(&format!("an unknown phase {other}"))),
    }
}

/// A text a reader returned: present, within `max` characters, and without control characters.
fn checked_text(text: &str, max: usize, what: &str) -> Result<(), String> {
    if text.is_empty() || text.chars().count() > max || text.chars().any(char::is_control) {
        return Err(format!(
            "The reader returned a {what} that DevHQ cannot accept."
        ));
    }
    Ok(())
}

/// A moment a reader returned, which DevHQ must be able to show.
fn checked_time(ms: Option<i64>, what: &str) -> Result<(), String> {
    match ms {
        Some(ms) if crate::records::format_time(ms).is_none() => Err(format!(
            "The reader returned a {what} that is no moment DevHQ can show."
        )),
        _ => Ok(()),
    }
}

/// The refs a reader returned, if they hold together: the reader is DevHQ's own code, but it read
/// hostile data, so what it says is checked as untrusted.
fn validated_refs(refs: Refs) -> Result<Refs, String> {
    if !OUTCOMES.contains(&refs.outcome.as_str()) {
        return Err("The reader returned an unknown outcome.".into());
    }
    if refs.outcome != "read" {
        let only_why = Refs {
            outcome: refs.outcome.clone(),
            detail: refs.detail.clone(),
            ..Refs::default()
        };
        let Some(detail) = &refs.detail else {
            return Err("The reader did not say why it could not read the copy.".into());
        };
        checked_text(detail, reader::MAX_DETAIL, "reason")?;
        if refs != only_why {
            return Err("The reader returned facts of a copy it did not read.".into());
        }
        return Ok(refs);
    }
    if refs.detail.is_some() {
        return Err("The reader returned a reason for a copy it read.".into());
    }
    let (Some(on_head), Some(elsewhere), Some(_), Some(remotes)) = (
        refs.unpublished_on_head,
        refs.unpublished_elsewhere,
        refs.stashes,
        &refs.remotes,
    ) else {
        return Err("The reader left out facts of a copy it read.".into());
    };
    if on_head > UNPUBLISHED_LIMIT + 1 || elsewhere > UNPUBLISHED_LIMIT + 1 {
        return Err("The reader counted more unpublished commits than it counts.".into());
    }
    if let Some(branch) = &refs.branch {
        checked_text(branch, reader::MAX_NAME, "branch")?;
    }
    if let Some(head) = &refs.head {
        if head.len() != 40
            || !head
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err("The reader returned a HEAD that is no commit ID.".into());
        }
    } else if on_head > 0 {
        return Err("The reader counted unpublished commits on a HEAD without commits.".into());
    }
    checked_time(refs.last_commit_at, "last commit time")?;
    checked_time(refs.synced_at, "last contact with a remote")?;
    if remotes.len() > 20 {
        return Err("The reader returned more remotes than it keeps.".into());
    }
    for remote in remotes {
        checked_text(&remote.name, reader::MAX_NAME, "remote name")?;
        // Kept only as `remotes` cleans it: no credentials can slip through.
        if let Some(url) = &remote.url {
            if remotes::sanitized(url).as_deref() != Some(url.as_str()) {
                return Err("The reader returned a remote URL DevHQ does not keep.".into());
            }
        }
    }
    if let Some(operation) = &refs.operation {
        if !OPERATIONS.contains(&operation.as_str()) {
            return Err("The reader returned an unknown operation.".into());
        }
    }
    Ok(refs)
}

fn validated_worktree(worktree: Worktree) -> Result<Worktree, String> {
    let counted = [worktree.changed, worktree.untracked, worktree.unverifiable];
    match (&worktree.unknown, counted) {
        (None, [Some(_), Some(_), Some(_)]) => Ok(worktree),
        (Some(reason), [None, None, None]) => {
            checked_text(reason, reader::MAX_DETAIL, "reason")?;
            Ok(worktree)
        }
        _ => Err("The reader returned a working tree that is neither counted nor unknown.".into()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    /// The environment variable that makes `serves_as_the_reader` a reader, and how.
    const SERVE: &str = "DEVHQ_TEST_READER";

    /// Run by the tests below in a process of its own, as DevHQ runs `devhq --read-copy`; on its
    /// own, it does nothing. `confined` serves as the real reader does; `unconfined` reads without
    /// forbidding programs, to show that libgit2 itself starts none; `probe` confines itself and
    /// then tries to start the program its request names, answering whether it could.
    #[test]
    #[ignore = "a helper that other tests run in a process of their own"]
    fn serves_as_the_reader() {
        let Some(mode) = std::env::var_os(SERVE) else {
            return;
        };
        let (stdin, stdout) = (std::io::stdin().lock(), std::io::stdout().lock());
        let code = match mode.to_str() {
            Some("confined") => reader::serve(stdin, stdout, reader::Confinement::Required),
            Some("unconfined") => reader::serve(stdin, stdout, reader::Confinement::Skipped),
            Some("probe") => probe(stdin, stdout),
            Some(misbehaving) => misbehave(misbehaving, stdout),
            None => 2,
        };
        // Before the harness prints anything more on stdout.
        std::process::exit(code);
    }

    /// A reader that misbehaves as `how` says, as one stuck or broken by hostile data would.
    fn misbehave(how: &str, mut output: impl Write) -> i32 {
        let refs = r#"{"phase":"refs","outcome":"read","detail":null,"branch":"main","head":null,"lastCommitAt":null,"unpublishedOnHead":0,"unpublishedElsewhere":0,"stashes":0,"syncedAt":null,"remotes":[],"operation":null}"#;
        match how {
            // Its refs, and then nothing more: stuck in the working tree.
            "stall" => {
                writeln!(output, "{refs}").unwrap();
                output.flush().unwrap();
                std::thread::sleep(Duration::from_secs(60));
            }
            // Nothing at all.
            "silent" => std::thread::sleep(Duration::from_secs(60)),
            // Gone without a word.
            "crash" => std::process::abort(),
            "lie" => writeln!(
                output,
                "{}",
                refs.replace(r#""head":null"#, r#""head":"HEAD""#)
            )
            .unwrap(),
            "flood" => {
                let flood = "x".repeat(2 * 1024 * 1024);
                writeln!(
                    output,
                    r#"{{"phase":"refs","outcome":"missing","detail":"{flood}"}}"#
                )
                .unwrap();
            }
            _ => return 2,
        }
        0
    }

    #[test]
    fn meets_its_deadline_and_refuses_what_it_cannot_accept() {
        let quick = Duration::from_secs(3);
        let started = Instant::now();
        let stalled = test_reader("stall", &[]).read("/anywhere", quick).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "it waited for the reader"
        );
        assert_eq!(stalled.refs.branch.as_deref(), Some("main"));
        assert_eq!(
            stalled.worktree.unwrap().unknown.as_deref(),
            Some("Reading the working tree took longer than 3 s, so DevHQ stopped it.")
        );
        for (how, message) in [
            (
                "silent",
                "took longer than 3 s, so DevHQ stopped it. Nothing was kept.",
            ),
            ("crash", "The reader stopped without an answer"),
            ("lie", "a HEAD that is no commit ID"),
            ("flood", "cannot accept"),
        ] {
            let error = test_reader(how, &[]).read("/anywhere", quick).unwrap_err();
            assert_eq!(error.code, "read_error");
            assert!(error.message.contains(message), "{how}: {}", error.message);
        }
    }

    fn probe(input: impl Read, mut output: impl Write) -> i32 {
        let mut request = String::new();
        input.take(MAX_ANSWER).read_to_string(&mut request).unwrap();
        let program: Value = serde_json::from_str(&request).unwrap();
        let confined = confine::forbid_programs();
        let started = Command::new(program["path"].as_str().unwrap())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let answer = serde_json::json!({
            "confined": confined.err(),
            "started": started.as_ref().map(|status| status.code()).map_err(|error| error.to_string()),
        });
        writeln!(output, "{answer}").unwrap();
        0
    }

    /// This test executable, running only `serves_as_the_reader` as `mode`, with `envs`.
    pub(crate) fn test_reader(mode: &str, envs: &[(&str, &OsString)]) -> Reader {
        let mut all: Vec<(OsString, OsString)> = vec![(SERVE.into(), mode.into())];
        all.extend(
            envs.iter()
                .map(|(key, value)| (OsString::from(key), (*value).clone())),
        );
        Reader {
            program: std::env::current_exe().unwrap(),
            args: [
                "--ignored",
                "--exact",
                "local_copies::tests::serves_as_the_reader",
                "--nocapture",
                "-q",
            ]
            .map(OsString::from)
            .to_vec(),
            envs: all,
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

    /// No global Git config, so that the tests' `git` never reads the machine's own: Git skips a
    /// global config that is not there, and this one cannot be, in a folder nobody creates. No
    /// file is left behind in the temporary folder, which the Windows workflow checks.
    fn no_config() -> PathBuf {
        crate::inventory::tests::resolved_temp_dir()
            .join("devhq-no-such-folder")
            .join("gitconfig")
    }

    /// Runs the installed `git` with `args` in `dir`, as a person would, without the machine's
    /// config: its output, or None when it fails. Tests build repositories with it; DevHQ never
    /// runs it.
    pub(crate) fn try_git(dir: &Path, args: &[&str]) -> Option<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "DevHQ test")
            .env("GIT_AUTHOR_EMAIL", "test@devhq.invalid")
            .env("GIT_COMMITTER_NAME", "DevHQ test")
            .env("GIT_COMMITTER_EMAIL", "test@devhq.invalid")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", no_config())
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout).unwrap())
    }

    /// Like `try_git`, for a command that must succeed.
    pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
        try_git(dir, args).unwrap_or_else(|| panic!("git {args:?} failed in {}", dir.display()))
    }

    pub(crate) fn path_text(path: &Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn says_what_is_at_a_path_that_holds_no_copy() {
        let folder = folder("not-a-copy");
        let reader = test_reader("confined", &[]);
        let file = folder.join("notes.txt");
        std::fs::write(&file, "x").unwrap();
        let inner = folder.join("repo");
        std::fs::create_dir(&inner).unwrap();
        git(&inner, &["init", "-q", "-b", "main"]);
        std::fs::create_dir(inner.join("sub")).unwrap();
        let bare = folder.join("bare.git");
        std::fs::create_dir(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare"]);
        let empty = folder.join("empty");
        std::fs::create_dir(&empty).unwrap();
        for (path, outcome, detail) in [
            (
                folder.join("nothing"),
                "missing",
                "Nothing is at this path.",
            ),
            (file, "not_a_copy", "This is a file"),
            (empty, "not_a_copy", "No repository starts at this folder"),
            // Never the repository a folder is in: only the one that starts there.
            (
                inner.join("sub"),
                "not_a_copy",
                "No repository starts at this folder",
            ),
            (bare, "not_a_copy", "A bare repository"),
        ] {
            let read = reader.read(&path_text(&path), DEADLINE).unwrap();
            assert_eq!(read.refs.outcome, outcome, "{}", path.display());
            assert!(
                read.refs.detail.as_deref().unwrap().contains(detail),
                "{read:?}"
            );
            assert_eq!(read.worktree, None);
        }
        crate::inventory::tests::remove_now(folder);
    }

    /// What the installed `git` says of the copy at `dir`, as the reader would count it.
    fn as_git_reads(dir: &Path) -> (Refs, Worktree) {
        let lines = |args: &[&str]| -> Vec<String> {
            try_git(dir, args)
                .unwrap_or_default()
                .lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        };
        // Both fail when there is nothing to name: no commit yet, or a detached HEAD.
        let head = lines(&["rev-parse", "-q", "--verify", "HEAD"]).pop();
        let branch = lines(&["symbolic-ref", "-q", "--short", "HEAD"]).pop();
        let count = |args: &[&str]| -> u32 { lines(args)[0].parse().unwrap() };
        let others: Vec<String> = lines(&["for-each-ref", "--format=%(refname)", "refs/heads"])
            .into_iter()
            .filter(|name| Some(name.trim_start_matches("refs/heads/")) != branch.as_deref())
            .collect();
        let unpublished_elsewhere = if others.is_empty() {
            0
        } else {
            let mut args = vec!["rev-list", "--count"];
            args.extend(others.iter().map(String::as_str));
            args.extend(["--not", "--remotes"]);
            if head.is_some() {
                args.push("HEAD");
            }
            count(&args)
        };
        let mut times: Vec<i64> = lines(&[
            "for-each-ref",
            "--format=%(committerdate:unix)",
            "refs/heads",
        ])
        .iter()
        .map(|time| time.parse::<i64>().unwrap() * 1000)
        .collect();
        if head.is_some() {
            times.push(
                lines(&["log", "-1", "--format=%ct"])[0]
                    .parse::<i64>()
                    .unwrap()
                    * 1000,
            );
        }
        let refs = Refs {
            outcome: "read".into(),
            branch,
            unpublished_on_head: Some(if head.is_some() {
                count(&["rev-list", "--count", "HEAD", "--not", "--remotes"])
            } else {
                0
            }),
            head,
            last_commit_at: times.into_iter().max(),
            unpublished_elsewhere: Some(unpublished_elsewhere),
            stashes: Some(lines(&["stash", "list"]).len() as u32),
            ..Refs::default()
        };
        let status = lines(&[
            "status",
            "--porcelain=v1",
            "--untracked-files=normal",
            "--ignore-submodules=all",
        ]);
        let untracked = status.iter().filter(|line| line.starts_with("??")).count() as u32;
        let worktree = Worktree {
            changed: Some(status.len() as u32 - untracked),
            untracked: Some(untracked),
            unverifiable: Some(0),
            unknown: None,
        };
        (refs, worktree)
    }

    /// The reading, without what `as_git_reads` does not ask `git` for.
    fn comparable(read: &Facts) -> (Refs, Worktree) {
        let refs = Refs {
            synced_at: None,
            remotes: None,
            operation: None,
            ..read.refs.clone()
        };
        (refs, read.worktree.clone().unwrap())
    }

    fn commit(dir: &Path, file: &str, text: &str, message: &str) {
        std::fs::write(dir.join(file), text).unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "-q", "-m", message]);
    }

    #[test]
    fn reads_what_the_installed_git_reads() {
        let root = folder("twins");
        let reader = test_reader("confined", &[]);
        let read = |dir: &Path| reader.read(&path_text(dir), DEADLINE).unwrap();
        let fresh = |name: &str| {
            let dir = root.join(name);
            std::fs::create_dir(&dir).unwrap();
            git(&dir, &["init", "-q", "-b", "main"]);
            dir
        };
        let origin = root.join("origin.git");
        std::fs::create_dir(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);

        // Published and unpublished work on two branches, stashes, changes of every kind and
        // new entries, a folder of them counting once.
        let shop = fresh("shop");
        commit(&shop, "a.txt", "a\n", "a");
        git(&shop, &["remote", "add", "origin", &path_text(&origin)]);
        git(&shop, &["push", "-q", "origin", "main"]);
        commit(&shop, "b.txt", "b\n", "b");
        commit(&shop, "c.txt", "c\n", "c");
        git(&shop, &["checkout", "-q", "-b", "topic"]);
        commit(&shop, "d.txt", "d\n", "d");
        git(&shop, &["checkout", "-q", "main"]);
        for stash in ["one", "two"] {
            std::fs::write(shop.join("a.txt"), stash).unwrap();
            git(&shop, &["stash", "-q"]);
        }
        std::fs::write(shop.join("a.txt"), "changed\n").unwrap();
        std::fs::write(shop.join("b.txt"), "staged\n").unwrap();
        git(&shop, &["add", "b.txt"]);
        std::fs::remove_file(shop.join("c.txt")).unwrap();
        std::fs::write(shop.join("new.txt"), "n\n").unwrap();
        std::fs::create_dir_all(shop.join("new").join("deeper")).unwrap();
        std::fs::write(shop.join("new").join("deeper").join("x.txt"), "x\n").unwrap();
        let expected = as_git_reads(&shop);
        assert_eq!(expected.0.unpublished_on_head, Some(2));
        assert_eq!(expected.0.unpublished_elsewhere, Some(1));
        let reading = read(&shop);
        assert_eq!(comparable(&reading), expected);
        assert_eq!(reading.refs.operation, None);
        assert!(reading.refs.synced_at.is_some());

        // Detached HEAD: no branch, and every local branch is elsewhere.
        git(&shop, &["stash", "-q", "-u"]);
        git(&shop, &["checkout", "-q", "--detach", "HEAD~1"]);
        let expected = as_git_reads(&shop);
        assert_eq!(expected.0.branch, None);
        assert_eq!(comparable(&read(&shop)), expected);

        // No commit yet: the branch it will be on, no HEAD, and new entries.
        let unborn = fresh("unborn");
        std::fs::write(unborn.join("x.txt"), "x\n").unwrap();
        let expected = as_git_reads(&unborn);
        assert_eq!(expected.0.head, None);
        assert_eq!(comparable(&read(&unborn)), expected);

        // A merge left halfway, with its conflict counted as a change.
        let merge = fresh("merge");
        commit(&merge, "x.txt", "base\n", "base");
        git(&merge, &["checkout", "-q", "-b", "other"]);
        commit(&merge, "x.txt", "other\n", "other");
        git(&merge, &["checkout", "-q", "main"]);
        commit(&merge, "x.txt", "main\n", "main");
        assert_eq!(
            try_git(&merge, &["merge", "-q", "other"]),
            None,
            "the merge must stop on its conflict"
        );
        let expected = as_git_reads(&merge);
        let reading = read(&merge);
        assert_eq!(comparable(&reading), expected);
        assert_eq!(reading.refs.operation.as_deref(), Some("merge"));
        git(&merge, &["merge", "--abort"]);

        // A rebase left halfway.
        commit(&merge, "y.txt", "y\n", "y");
        assert_eq!(
            try_git(&merge, &["rebase", "-q", "other"]),
            None,
            "the rebase must stop on its conflict"
        );
        assert_eq!(read(&merge).refs.operation.as_deref(), Some("rebase"));

        // A file under a filter driver the repository defines, touched without changing: Git runs
        // the driver and sees it clean; DevHQ does not run it, so it cannot verify it.
        let filtered = fresh("filtered");
        std::fs::write(filtered.join(".gitattributes"), "*.up filter=upper\n").unwrap();
        git(&filtered, &["config", "filter.upper.clean", "sed s/^/X/"]);
        git(&filtered, &["config", "filter.upper.smudge", "sed s/^X//"]);
        std::fs::write(filtered.join("a.up"), "a\n").unwrap();
        git(&filtered, &["add", "-A"]);
        git(&filtered, &["commit", "-q", "-m", "filtered"]);
        std::fs::File::options()
            .write(true)
            .open(filtered.join("a.up"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + Duration::from_secs(120))
            .unwrap();
        let expected = as_git_reads(&filtered);
        assert_eq!(expected.1.changed, Some(0), "git ran the driver");
        let reading = read(&filtered);
        let worktree = reading.worktree.unwrap();
        assert_eq!(
            (worktree.changed, worktree.untracked, worktree.unverifiable),
            (Some(0), Some(0), Some(1))
        );
        crate::inventory::tests::remove_now(root);
    }

    /// A ref's log is read as Git reads it, whatever its length, or not at all: the newest entry
    /// from the log's end, so a long log never gives an older moment; entries counted only in a log
    /// read whole, so a longer one is a failure of DevHQ, never a smaller count; and a log the
    /// system does not let DevHQ read leaves the copy unreadable, never with no stash or no sync.
    #[test]
    fn reads_a_log_of_any_length_as_git_does_or_says_it_cannot() {
        let root = folder("logs");
        let reader = test_reader("confined", &[]);
        let read = |dir: &Path| reader.read(&path_text(dir), DEADLINE);
        let origin = root.join("origin.git");
        std::fs::create_dir(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        let copy = root.join("copy");
        std::fs::create_dir(&copy).unwrap();
        git(&copy, &["init", "-q", "-b", "main"]);
        commit(&copy, "a.txt", "a\n", "a");
        git(&copy, &["remote", "add", "origin", &path_text(&origin)]);
        // A push logs the remote branch it moves, and writes no FETCH_HEAD: the log is the only
        // sign of contact.
        git(&copy, &["push", "-q", "origin", "main"]);
        assert!(!copy.join(".git").join("FETCH_HEAD").exists());
        let head = git(&copy, &["rev-parse", "HEAD"]).trim().to_owned();
        let entry = |seconds: i64| {
            format!("{head} {head} DevHQ test <test@devhq.invalid> {seconds} +0000\tpush\n")
        };
        let longer_than_read = |line: &str| line.repeat(reader::MAX_LOG as usize / line.len() + 1);

        // A remote branch's log longer than DevHQ reads whole, its newest entry last, then a line
        // still being written, which Git skips.
        let pushed = copy.join(".git/logs/refs/remotes/origin/main");
        let mut log = longer_than_read(&entry(1_000_000_000));
        log.push_str(&entry(1_700_000_000));
        log.push_str(&head);
        std::fs::write(&pushed, &log).unwrap();
        let reading = read(&copy).unwrap();
        assert_eq!(reading.refs.synced_at, Some(1_700_000_000_000));
        // An end with no entry Git would read leaves the newest unknown: nothing is kept.
        log.push_str(&format!("\n{}\n", "x".repeat(reader::TAIL as usize)));
        std::fs::write(&pushed, &log).unwrap();
        let error = read(&copy).unwrap_err();
        assert_eq!(error.code, "read_error");
        assert!(
            error.message.contains("found no entry in the last 1 MiB"),
            "{}",
            error.message
        );
        std::fs::write(&pushed, entry(1_700_000_000)).unwrap();

        // Stashes, counted as Git lists them: a line that is no entry is skipped.
        std::fs::write(copy.join("a.txt"), "stashed\n").unwrap();
        git(&copy, &["stash", "-q"]);
        let stash = copy.join(".git/logs/refs/stash");
        let stashed = std::fs::read_to_string(&stash).unwrap();
        std::fs::write(&stash, format!("{stashed}not an entry\n")).unwrap();
        assert_eq!(read(&copy).unwrap().refs.stashes, Some(1));
        assert_eq!(as_git_reads(&copy).0.stashes, Some(1));
        // Longer than DevHQ reads whole: no count rather than the count of its beginning.
        std::fs::write(&stash, longer_than_read(&stashed)).unwrap();
        let error = read(&copy).unwrap_err();
        assert_eq!(error.code, "read_error");
        assert!(
            error.message.contains("longer than the 32 MiB DevHQ reads"),
            "{}",
            error.message
        );
        // No log at all: no stash, as Git lists none.
        std::fs::remove_file(&stash).unwrap();
        assert_eq!(read(&copy).unwrap().refs.stashes, Some(0));
        assert_eq!(as_git_reads(&copy).0.stashes, Some(0));
        std::fs::write(&stash, &stashed).unwrap();

        // Logs the system does not let DevHQ read.
        #[cfg(any(target_os = "linux", windows))]
        if let Some(allow) = forbid(&copy.join(".git").join("logs")) {
            let reading = read(&copy);
            drop(allow);
            let reading = reading.unwrap();
            assert_eq!(reading.refs.outcome, "unreadable", "{reading:?}");
            assert!(
                reading
                    .refs
                    .detail
                    .as_deref()
                    .unwrap()
                    .starts_with("The system did not let DevHQ read the log of refs/"),
                "{reading:?}"
            );
            assert_eq!(reading.worktree, None);
        }
        assert_eq!(read(&copy).unwrap().refs.stashes, Some(1));
        crate::inventory::tests::remove_now(root);
    }

    #[test]
    fn a_format_libgit2_does_not_know_is_a_failure_of_devhq_not_a_fact() {
        let root = folder("formats");
        let reader = test_reader("confined", &[]);
        // Refs in a format libgit2 cannot read: nothing to keep of the copy.
        let reftable = root.join("reftable");
        std::fs::create_dir(&reftable).unwrap();
        git(&reftable, &["init", "-q", "-b", "main"]);
        commit(&reftable, "a.txt", "a\n", "a");
        git(&reftable, &["config", "core.repositoryformatversion", "1"]);
        git(&reftable, &["config", "extensions.refStorage", "reftable"]);
        let error = reader.read(&path_text(&reftable), DEADLINE).unwrap_err();
        assert_eq!(error.code, "read_error");
        assert!(
            error.message.contains("libgit2 could not open this copy"),
            "{}",
            error.message
        );
        // An index split in two, which libgit2 cannot read: the refs are kept, the working tree is
        // unknown.
        let split = root.join("split");
        std::fs::create_dir(&split).unwrap();
        git(&split, &["init", "-q", "-b", "main"]);
        commit(&split, "a.txt", "a\n", "a");
        git(&split, &["update-index", "--split-index"]);
        let reading = reader.read(&path_text(&split), DEADLINE).unwrap();
        assert_eq!(reading.refs.outcome, "read");
        let worktree = reading.worktree.unwrap();
        assert_eq!(worktree.changed, None);
        assert!(
            worktree
                .unknown
                .as_deref()
                .unwrap()
                .starts_with("libgit2 could not read the working tree"),
            "{worktree:?}"
        );
        crate::inventory::tests::remove_now(root);
    }

    /// Keeps this user from looking into `dir` (`chmod 000`) until the guard drops. None when
    /// nothing would: root looks anyway.
    #[cfg(target_os = "linux")]
    fn forbid(dir: &Path) -> Option<impl Drop> {
        use std::os::unix::fs::PermissionsExt;
        struct Allow(PathBuf);
        impl Drop for Allow {
            fn drop(&mut self) {
                std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return None;
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        Some(Allow(dir.to_owned()))
    }

    /// Keeps this user from looking into `dir` with an ACL entry that denies them reading it and
    /// what it holds, until the guard drops. The owner may always change the ACL back.
    #[cfg(windows)]
    fn forbid(dir: &Path) -> Option<impl Drop> {
        struct Allow(PathBuf, String);
        impl Drop for Allow {
            fn drop(&mut self) {
                let grant = format!("*{}", self.1);
                let status = Command::new("icacls")
                    .arg(&self.0)
                    .args(["/remove:d", &grant])
                    .output()
                    .unwrap()
                    .status;
                assert!(status.success());
            }
        }
        let whoami = Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .unwrap();
        let sid = String::from_utf8(whoami.stdout)
            .unwrap()
            .trim()
            .rsplit(',')
            .next()
            .unwrap()
            .trim_matches('"')
            .to_owned();
        assert!(sid.starts_with("S-1-"), "{sid}");
        let status = Command::new("icacls")
            .arg(dir)
            .args(["/deny", &format!("*{sid}:(OI)(CI)(RX)")])
            .output()
            .unwrap()
            .status;
        assert!(status.success());
        Some(Allow(dir.to_owned(), sid))
    }

    /// A copy this user may not look into is unreadable, with the system's reason, and never "no
    /// copy here": libgit2 takes every path it may not look at for one that is not there.
    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn a_copy_the_system_does_not_let_this_user_read_is_unreadable() {
        let root = folder("forbidden");
        let reader = test_reader("confined", &[]);
        // The folder itself, and only the repository inside a folder this user may read.
        for forbidden in ["", ".git"] {
            let copy = root.join(format!("copy{}", forbidden.len()));
            std::fs::create_dir(&copy).unwrap();
            git(&copy, &["init", "-q", "-b", "main"]);
            commit(&copy, "a.txt", "a\n", "a");
            let Some(allow) = forbid(&copy.join(forbidden)) else {
                return;
            };
            let reading = reader.read(&path_text(&copy), DEADLINE);
            drop(allow);
            let reading = reading.unwrap();
            assert_eq!(
                reading.refs.outcome, "unreadable",
                "{forbidden}: {reading:?}"
            );
            // The system's reason, whoever met the denial: DevHQ looking again after libgit2 found
            // nothing, or libgit2 itself when the system let it see `.git` but not open its config
            // (as on the Windows runner under a TMP with spaces and accents). The system's words
            // are in its own language, so only DevHQ's and libgit2's are checked.
            let detail = reading.refs.detail.as_deref().unwrap();
            assert!(
                detail.starts_with("The system did not let DevHQ look")
                    || detail.starts_with("could not open"),
                "{forbidden}: {reading:?}"
            );
            // Allowed again, the same copy reads.
            let reading = reader.read(&path_text(&copy), DEADLINE).unwrap();
            assert_eq!(reading.refs.outcome, "read", "{forbidden}: {reading:?}");
        }
        crate::inventory::tests::remove_now(root);
    }

    /// On Windows, Git for Windows' own config usually sets `core.autocrlf=true`, and the person's
    /// Git then sees a file checked out with CRLF as unchanged. The confined reader, with the
    /// environment DevHQ gives it, must find that config and agree.
    #[test]
    fn reads_line_endings_as_the_systems_git_config_says() {
        let system = Command::new("git")
            .args(["config", "--system", "--get", "core.autocrlf"])
            .output()
            .unwrap();
        if String::from_utf8_lossy(&system.stdout).trim() != "true" {
            // Linux, or a Git for Windows installed without it: nothing to agree on.
            return;
        }
        let dir = folder("autocrlf");
        git(&dir, &["init", "-q", "-b", "main"]);
        commit(&dir, "a.txt", "one\ntwo\n", "lf");
        // Checked out again as Git for Windows checks out: with CRLF, which the index records.
        std::fs::remove_file(dir.join("a.txt")).unwrap();
        git(
            &dir,
            &["-c", "core.autocrlf=true", "checkout", "--", "a.txt"],
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt")).unwrap(),
            "one\r\ntwo\r\n"
        );
        // Modified at another moment than the index says, so that its content is compared.
        std::fs::File::options()
            .write(true)
            .open(dir.join("a.txt"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + Duration::from_secs(120))
            .unwrap();
        let reading = test_reader("confined", &[])
            .read(&path_text(&dir), DEADLINE)
            .unwrap();
        assert_eq!(reading.worktree.unwrap().changed, Some(0));
        crate::inventory::tests::remove_now(dir);
    }

    #[test]
    fn a_confined_reader_cannot_start_any_program() {
        // A program that would leave a mark if it ever ran: the shell on Linux, cmd on Windows.
        let program = if cfg!(windows) {
            std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into())
        } else {
            "/bin/sh".into()
        };
        let probe = |mode: &str| -> Value {
            let reader = test_reader(mode, &[]);
            let mut child = Command::new(&reader.program)
                .args(&reader.args)
                .envs(reader.envs.iter().map(|(key, value)| (key, value)))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(
                    serde_json::json!({ "path": program })
                        .to_string()
                        .as_bytes(),
                )
                .unwrap();
            let output = child.wait_with_output().unwrap();
            let text = String::from_utf8(output.stdout).unwrap();
            let line = text.lines().find(|line| line.starts_with('{')).unwrap();
            serde_json::from_str(line).unwrap()
        };
        let confined = probe("probe");
        assert_eq!(confined["confined"], Value::Null, "{confined}");
        assert!(confined["started"]["Err"].is_string(), "{confined}");
    }

    #[test]
    fn refuses_an_answer_that_does_not_hold_together() {
        let read = Refs {
            outcome: "read".into(),
            unpublished_on_head: Some(0),
            unpublished_elsewhere: Some(0),
            stashes: Some(0),
            remotes: Some(vec![]),
            ..Refs::default()
        };
        assert!(validated_refs(read.clone()).is_ok());
        let refused: Vec<(Refs, &str)> = vec![
            (
                Refs {
                    outcome: "deleted".into(),
                    ..read.clone()
                },
                "unknown outcome",
            ),
            (
                Refs {
                    outcome: "missing".into(),
                    ..read.clone()
                },
                "did not say why",
            ),
            (
                Refs {
                    outcome: "missing".into(),
                    detail: Some("gone".into()),
                    ..read.clone()
                },
                "facts of a copy it did not read",
            ),
            (
                Refs {
                    stashes: None,
                    ..read.clone()
                },
                "left out facts",
            ),
            (
                Refs {
                    unpublished_on_head: Some(UNPUBLISHED_LIMIT + 2),
                    ..read.clone()
                },
                "more unpublished",
            ),
            (
                Refs {
                    unpublished_on_head: Some(1),
                    ..read.clone()
                },
                "HEAD without commits",
            ),
            (
                Refs {
                    head: Some("HEAD".into()),
                    ..read.clone()
                },
                "no commit ID",
            ),
            (
                Refs {
                    branch: Some("main\u{1b}[31m".into()),
                    ..read.clone()
                },
                "branch",
            ),
            (
                Refs {
                    last_commit_at: Some(i64::MAX),
                    ..read.clone()
                },
                "no moment",
            ),
            (
                Refs {
                    remotes: Some(vec![Remote {
                        name: "origin".into(),
                        url: Some("https://me:token@github.com/me/shop".into()),
                    }]),
                    ..read.clone()
                },
                "remote URL",
            ),
            (
                Refs {
                    remotes: Some(
                        (0..21)
                            .map(|index| Remote {
                                name: format!("r{index}"),
                                url: None,
                            })
                            .collect(),
                    ),
                    ..read.clone()
                },
                "more remotes",
            ),
            (
                Refs {
                    operation: Some("rm -rf".into()),
                    ..read.clone()
                },
                "unknown operation",
            ),
        ];
        for (refs, message) in refused {
            let error = validated_refs(refs).unwrap_err();
            assert!(error.contains(message), "{error}");
        }
        assert!(validated_worktree(Worktree {
            changed: Some(1),
            untracked: None,
            unverifiable: Some(0),
            unknown: None,
        })
        .is_err());
        assert!(validated_worktree(Worktree::unknown("slow")).is_ok());
        // A line that is not the protocol is refused, not guessed at.
        for line in [
            r#"{"phase":"refs","outcome":"read","extra":1}"#,
            r#"{"outcome":"read"}"#,
            r#"{"phase":"later"}"#,
            "{",
        ] {
            assert!(answer_line(line).is_err(), "{line}");
        }
    }
}
