//! What runs inside `devhq --read-copy`: one local copy read with libgit2, in a process of its own
//! that first forbids itself to start programs (see `confine`).
//!
//! It reads one JSON object from stdin, `{"path": "…"}`, and answers on stdout with one JSON line
//! per phase: first `refs` (whether a copy is there, its branch, HEAD, activity, unpublished
//! commits, stashes, last contact with a remote, remotes and any operation left halfway), then
//! `worktree` (how many tracked files changed, how many new entries, how many DevHQ cannot verify),
//! so that if the working tree takes too long the rest has already arrived. A failure of DevHQ
//! itself (it could not confine itself, libgit2 cannot read this format) is an `error` line.
//!
//! Only libgit2's read APIs are called. The repository is opened at exactly the declared path
//! (`NO_SEARCH`), its status is read without updating the index, and nothing is ever written: no
//! checkout, fetch, ref, index, log or config. Not every libgit2 API that sounds like reading only
//! reads: reading a ref's log creates an empty one when there is none, so logs are read as files.
//! The hostile battery holds every call to this. libgit2 applies its built-in filters (crlf,
//! ident) and never runs the external drivers, hooks, fsmonitor, signing or remote helper programs
//! a repository names. A file that looks changed under a driver the config defines is counted
//! apart, as unverifiable, instead of as a change or as clean.

use super::remotes;
use super::{Refs, Remote, Worktree, UNPUBLISHED_LIMIT};
use git2::{
    AttrCheckFlags, AttrValue, BranchType, Commit, ErrorClass, ErrorCode, Oid, Repository,
    RepositoryOpenFlags, RepositoryState, Status, StatusOptions,
};
use serde_json::{json, Value};
use std::ffi::OsStr;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Remotes a reading keeps: more says nothing more about a copy.
const MAX_REMOTES: usize = 20;
/// The longest branch or remote name kept.
pub(crate) const MAX_NAME: usize = 255;
/// The longest reason kept for a copy that could not be read, or a working tree that could not.
pub(crate) const MAX_DETAIL: usize = 1000;
/// The request is one short JSON object.
const MAX_REQUEST: u64 = 64 * 1024;

/// Whether the reader must forbid itself to start programs before reading. Only tests read
/// without it, to show that libgit2 itself starts none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Confinement {
    Required,
    #[cfg(test)]
    Skipped,
}

/// Serves one request from `input` on `output`; returns the process exit code.
pub(crate) fn serve(input: impl Read, mut output: impl Write, confinement: Confinement) -> i32 {
    let mut answer = |line: Value| -> bool {
        writeln!(output, "{line}")
            .and_then(|()| output.flush())
            .is_ok()
    };
    if confinement == Confinement::Required {
        if let Err(message) = super::confine::forbid_programs() {
            answer(json!({ "phase": "error", "message": message }));
            return 1;
        }
    }
    let path = match request(input) {
        Ok(path) => path,
        Err(message) => {
            answer(json!({ "phase": "error", "message": message }));
            return 1;
        }
    };
    let (refs, repository) = match read_refs(Path::new(&path)) {
        Ok(read) => read,
        Err(message) => {
            answer(json!({ "phase": "error", "message": message }));
            return 1;
        }
    };
    let mut line = json!(refs);
    line["phase"] = json!("refs");
    if !answer(line) {
        return 1;
    }
    if let Some(repository) = repository {
        let mut line = json!(read_worktree(&repository));
        line["phase"] = json!("worktree");
        if !answer(line) {
            return 1;
        }
    }
    0
}

/// The path the request names.
fn request(input: impl Read) -> Result<String, String> {
    let mut text = String::new();
    input
        .take(MAX_REQUEST)
        .read_to_string(&mut text)
        .map_err(|error| format!("The reader could not read its request: {error}"))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("The reader's request is not JSON: {error}"))?;
    value
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "The reader's request names no path.".to_owned())
}

/// Text as DevHQ keeps it from a repository it does not trust: control characters become spaces
/// and it is cut to `max` characters.
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

fn not_read(outcome: &str, detail: &str) -> Refs {
    Refs {
        outcome: outcome.to_owned(),
        detail: Some(plain_text(detail, MAX_DETAIL)),
        ..Refs::default()
    }
}

fn libgit2(error: &git2::Error) -> String {
    plain_text(error.message(), MAX_DETAIL)
}

/// The copy at `path`, as far as it is one: the facts of `refs`, and the repository to read the
/// working tree from when there is one. An error is a failure of DevHQ, not a fact of the copy.
pub(crate) fn read_refs(path: &Path) -> Result<(Refs, Option<Repository>), String> {
    match std::fs::metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok((not_read("missing", "Nothing is at this path."), None));
        }
        Err(error) => return Ok((not_read("unreadable", &error.to_string()), None)),
        Ok(metadata) if !metadata.is_dir() => {
            return Ok((
                not_read("not_a_copy", "This is a file, not the folder of a copy."),
                None,
            ));
        }
        Ok(_) => {}
    }
    let repository = match Repository::open_ext(
        path,
        RepositoryOpenFlags::NO_SEARCH,
        [] as [&OsStr; 0],
    ) {
        Ok(repository) => repository,
        Err(error) if error.code() == ErrorCode::NotFound => {
            return Ok((
                    not_read(
                        "not_a_copy",
                        "No repository starts at this folder. DevHQ does not look in the folders above it: declare the folder at the root of the copy.",
                    ),
                    None,
                ));
        }
        // A copy owned by another user, which Git itself would refuse (safe.directory), or
        // one the system does not let this user read.
        Err(error) if error.code() == ErrorCode::Owner || error.class() == ErrorClass::Os => {
            return Ok((not_read("unreadable", error.message()), None));
        }
        Err(error) => {
            return Err(format!(
                "libgit2 could not open this copy: {}",
                libgit2(&error)
            ))
        }
    };
    if repository.is_bare() {
        return Ok((
            not_read(
                "not_a_copy",
                "A bare repository, which has no working tree.",
            ),
            None,
        ));
    }
    let refs = facts(&repository)
        .map_err(|error| format!("libgit2 could not read this copy: {}", libgit2(&error)))?;
    Ok((refs, Some(repository)))
}

fn when(commit: &Commit) -> i64 {
    commit.time().seconds().saturating_mul(1000)
}

/// Everything but the working tree, from refs, commits, reflogs and config.
fn facts(repository: &Repository) -> Result<Refs, git2::Error> {
    let head_ref = repository.find_reference("HEAD")?;
    // HEAD names its branch, or is detached (no branch). Read as bytes: a hostile HEAD can name
    // anything.
    let branch_ref = head_ref.symbolic_target_bytes().map(<[u8]>::to_vec);
    // Unborn (no commit yet) leaves HEAD without a commit.
    let head = match repository.head() {
        Ok(reference) => Some(reference.peel_to_commit()?),
        Err(error) if error.code() == ErrorCode::UnbornBranch => None,
        Err(error) => return Err(error),
    };
    let branch = branch_ref.as_deref().map(|target| {
        let name = target.strip_prefix(b"refs/heads/").unwrap_or(target);
        plain_text(&String::from_utf8_lossy(name), MAX_NAME)
    });

    let mut last_commit_at = head.as_ref().map(when);
    let mut elsewhere: Vec<Oid> = Vec::new();
    for item in repository.branches(Some(BranchType::Local))? {
        let (local, _) = item?;
        let Ok(commit) = local.get().peel_to_commit() else {
            continue;
        };
        last_commit_at = last_commit_at.max(Some(when(&commit)));
        if Some(local.get().name_bytes()) != branch_ref.as_deref() {
            elsewhere.push(commit.id());
        }
    }

    // What the copy knows of its remotes: their branches, and when it last talked to one. A fetch
    // writes FETCH_HEAD (in the worktree's own folder, and in the shared one for the main
    // worktree); a fetch or a push logs each remote branch it moves.
    let mut published: Vec<Oid> = Vec::new();
    let mut synced_at = [repository.path(), repository.commondir()]
        .iter()
        .filter_map(|dir| {
            std::fs::metadata(dir.join("FETCH_HEAD"))
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        })
        .max();
    for item in repository.references_glob("refs/remotes/*")? {
        let reference = item?;
        // `refs/remotes/origin/HEAD` only names another remote branch.
        if reference.symbolic_target_bytes().is_some() {
            continue;
        }
        if let Ok(commit) = reference.peel_to_commit() {
            published.push(commit.id());
        }
        if let Ok(name) = reference.name() {
            synced_at = synced_at.max(log(repository, name).newest);
        }
    }

    let unpublished_on_head = unpublished(repository, head.iter().map(Commit::id), &published)?;
    let mut hidden = published.clone();
    hidden.extend(head.iter().map(Commit::id));
    let unpublished_elsewhere = unpublished(repository, elsewhere, &hidden)?;

    // Each stash is an entry of the stash's log.
    let stashes = if repository.find_reference("refs/stash").is_ok() {
        log(repository, "refs/stash").entries
    } else {
        0
    };

    let mut remotes = Vec::new();
    let names = repository.remotes()?;
    for name in names
        .iter()
        .filter_map(|name| name.ok().flatten())
        .take(MAX_REMOTES)
    {
        let url = repository
            .find_remote(name)
            .ok()
            .and_then(|remote| remote.url().ok().and_then(remotes::sanitized));
        remotes.push(Remote {
            name: plain_text(name, MAX_NAME),
            url,
        });
    }

    let operation = match repository.state() {
        RepositoryState::Clean => None,
        RepositoryState::Merge => Some("merge"),
        RepositoryState::Revert | RepositoryState::RevertSequence => Some("revert"),
        RepositoryState::CherryPick | RepositoryState::CherryPickSequence => Some("cherry_pick"),
        RepositoryState::Bisect => Some("bisect"),
        RepositoryState::Rebase
        | RepositoryState::RebaseInteractive
        | RepositoryState::RebaseMerge
        | RepositoryState::ApplyMailboxOrRebase => Some("rebase"),
        RepositoryState::ApplyMailbox => Some("apply_mailbox"),
    };

    Ok(Refs {
        outcome: "read".into(),
        detail: None,
        branch,
        head: head.as_ref().map(|commit| commit.id().to_string()),
        last_commit_at,
        unpublished_on_head: Some(unpublished_on_head),
        unpublished_elsewhere: Some(unpublished_elsewhere),
        stashes: Some(u32::try_from(stashes).unwrap_or(u32::MAX)),
        synced_at,
        remotes: Some(remotes),
        operation: operation.map(str::to_owned),
    })
}

/// What a ref's log says: how many entries, and when the newest was written.
#[derive(Default)]
struct Log {
    entries: usize,
    newest: Option<i64>,
}

/// The longest log DevHQ reads: a stash or a remote branch logs one line per change.
const MAX_LOG: u64 = 32 * 1024 * 1024;

/// The log of the shared ref `name` (a remote branch, the stash), read as the file it is. Not
/// with libgit2's reflog API, which creates an empty log file for a ref that has none (the hostile
/// battery found it): reading must never write. Each line is `old new committer when zone\tmessage`
/// and the newest comes last; a log that is missing or unreadable says nothing.
fn log(repository: &Repository, name: &str) -> Log {
    let Ok(file) = std::fs::File::open(repository.commondir().join("logs").join(name)) else {
        return Log::default();
    };
    let mut text = Vec::new();
    if file.take(MAX_LOG).read_to_end(&mut text).is_err() {
        return Log::default();
    }
    let lines: Vec<&[u8]> = text
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    let newest = lines.last().and_then(|line| {
        let head = line.split(|byte| *byte == b'\t').next()?;
        let mut fields = head.rsplit(|byte| *byte == b' ');
        let _zone = fields.next()?;
        let seconds: i64 = std::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
        Some(seconds.saturating_mul(1000))
    });
    Log {
        entries: lines.len(),
        newest,
    }
}

/// Commits reachable from `tips` that no `hidden` commit contains, counted up to one past the
/// limit, which reads as «more than the limit».
fn unpublished(
    repository: &Repository,
    tips: impl IntoIterator<Item = Oid>,
    hidden: &[Oid],
) -> Result<u32, git2::Error> {
    let mut walk = repository.revwalk()?;
    let mut any = false;
    for tip in tips {
        walk.push(tip)?;
        any = true;
    }
    if !any {
        return Ok(0);
    }
    for commit in hidden {
        // A remote branch whose commit is gone (a shallow or pruned copy) hides nothing.
        let _ = walk.hide(*commit);
    }
    let mut count = 0;
    for commit in walk {
        commit?;
        count += 1;
        if count > UNPUBLISHED_LIMIT {
            break;
        }
    }
    Ok(count)
}

/// The working tree as `git status` sees it, counted: tracked files with changes (staged or not,
/// conflicts included), new entries that are not ignored (a new folder counts once), and files
/// DevHQ cannot verify. Submodules are left out: each is a copy of its own, if declared.
pub(crate) fn read_worktree(repository: &Repository) -> Worktree {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(false)
        .include_ignored(false)
        .include_unmodified(false)
        .exclude_submodules(true)
        .update_index(false)
        .renames_head_to_index(false)
        .renames_index_to_workdir(false);
    let statuses = match repository.statuses(Some(&mut options)) {
        Ok(statuses) => statuses,
        Err(error) => {
            return Worktree::unknown(&format!(
                "libgit2 could not read the working tree: {}",
                error.message()
            ))
        }
    };
    let config = repository.config().ok();
    let (mut changed, mut untracked, mut unverifiable) = (0u32, 0u32, 0u32);
    for entry in statuses.iter() {
        let status = entry.status();
        if status.is_ignored() {
            continue;
        }
        if status == Status::WT_NEW {
            untracked += 1;
        } else if status == Status::WT_UNREADABLE
            || (status == Status::WT_MODIFIED
                && entry
                    .path()
                    .is_ok_and(|path| filtered(repository, config.as_ref(), path)))
        {
            // Unreadable, or looking changed only because a driver DevHQ does not run would
            // have cleaned it: neither a change nor clean.
            unverifiable += 1;
        } else {
            changed += 1;
        }
    }
    Worktree {
        changed: Some(changed),
        untracked: Some(untracked),
        unverifiable: Some(unverifiable),
        unknown: None,
    }
}

/// Whether `path` is under a filter driver that the config defines to clean files, which Git
/// would run before comparing and libgit2 does not. A `filter` attribute naming a driver the
/// config does not define is ignored by Git too, so the comparison stands.
fn filtered(repository: &Repository, config: Option<&git2::Config>, path: &str) -> bool {
    let Ok(value) =
        repository.get_attr_bytes(Path::new(path), "filter", AttrCheckFlags::FILE_THEN_INDEX)
    else {
        return false;
    };
    let AttrValue::String(driver) = AttrValue::from_bytes(value) else {
        return false;
    };
    config.is_some_and(|config| {
        ["clean", "process"]
            .iter()
            .any(|key| config.get_entry(&format!("filter.{driver}.{key}")).is_ok())
    })
}
