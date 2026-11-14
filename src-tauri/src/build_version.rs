// Which commit a build was made from, read from the checkout's `.git` as files, and the version a
// build declares with it. Included by `build.rs`, which writes that version into the Windows
// version resource, and compiled into the crate's tests, which hold it to every layout below.
//
// DevHQ asks the applications a person builds to declare their real version and their commit in
// their version resource («1.4.0+3f9c2ab»), so that a reading of the installed file says which
// build it is. This is DevHQ doing the same. It never runs `git`: a build script that started
// programs found on the PATH would run whatever is first there. It reads three or four small files
// and takes nothing else from the repository.
//
// What it cannot say, it does not say. Whether the tree had uncommitted changes when it was built
// cannot be known without comparing every file with the index, which is `git status`; so the
// version names the commit HEAD was at and claims nothing of the tree, unless whoever builds says
// it was dirty (`DEVHQ_BUILD_DIRTY`). And with no `.git` to read (a source archive), or one this
// does not understand, the version is the plain one: no commit is better than a guessed one.

use std::path::{Path, PathBuf};

/// A commit a checkout's HEAD names, and the files that change when it names another: the ones
/// that name it and, where Git keeps one, the log of HEAD's moves, since a new commit on a branch
/// whose ref was packed creates a file that was not there to watch. A build that depends on them
/// is made again when HEAD moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Head {
    /// The full object ID, in lowercase hexadecimal.
    pub commit: String,
    pub watch: Vec<PathBuf>,
}

/// An object ID as Git writes it: 40 hexadecimal digits (SHA-1) or 64 (SHA-256).
fn object_id(text: &str) -> Option<String> {
    let text = text.trim();
    (matches!(text.len(), 40 | 64) && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| text.to_ascii_lowercase())
}

/// A small text file, or None: these files are a line or a few kilobytes, never megabytes.
fn small_file(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    (size <= 4 * 1024 * 1024)
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()
}

/// The commit HEAD names in the checkout whose root is `checkout`, or None when there is no
/// repository there or this cannot tell: a missing or unreadable file, a ref that is nowhere, a
/// format it does not know (reftable).
///
/// `.git` is a folder in an ordinary checkout. In a linked worktree and in a submodule it is a
/// file, `gitdir: <path>`, that names the folder where that checkout's HEAD is; a worktree's
/// branches are then in the repository it is linked to, which its `commondir` file names.
pub(crate) fn head_of(checkout: &Path) -> Option<Head> {
    let dot_git = checkout.join(".git");
    let mut watch = Vec::new();
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let pointer = small_file(&dot_git)?;
        watch.push(dot_git);
        let named = pointer.lines().next()?.strip_prefix("gitdir:")?.trim();
        if named.is_empty() {
            return None;
        }
        // Relative to the checkout, as Git reads it.
        checkout.join(named)
    };
    let head_file = git_dir.join("HEAD");
    let head = small_file(&head_file)?;
    watch.push(head_file);
    // Never read: only watched.
    let log = git_dir.join("logs").join("HEAD");
    if log.is_file() {
        watch.push(log);
    }
    let head = head.trim();
    let Some(reference) = head.strip_prefix("ref:") else {
        // A detached HEAD is the commit itself.
        return Some(Head {
            commit: object_id(head)?,
            watch,
        });
    };
    let reference = reference.trim();
    // A ref is a path under the repository: nothing that climbs out of it is one.
    if !reference.starts_with("refs/")
        || reference
            .split('/')
            .any(|part| part.is_empty() || part == "..")
        || reference.contains('\\')
    {
        return None;
    }
    // Branches live in the common folder: the worktree's own, or the one it is linked to.
    let common = match small_file(&git_dir.join("commondir")) {
        Some(named) => {
            watch.push(git_dir.join("commondir"));
            git_dir.join(named.trim())
        }
        None => git_dir.clone(),
    };
    // Loose first, as Git resolves it: a ref can be both loose and packed, and the loose one wins.
    for folder in [&git_dir, &common] {
        let loose = folder.join(reference);
        if let Some(text) = small_file(&loose) {
            watch.push(loose);
            return Some(Head {
                commit: object_id(&text)?,
                watch,
            });
        }
    }
    let packed_file = common.join("packed-refs");
    let packed = small_file(&packed_file)?;
    watch.push(packed_file);
    // Lines of `<id> <ref>`; comments start with `#` and peeled tags with `^`.
    let commit = packed.lines().find_map(|line| {
        let (id, name) = line.split_once(' ')?;
        (name.trim() == reference).then(|| object_id(id)).flatten()
    })?;
    Some(Head { commit, watch })
}

/// The version a build declares: `version` alone without a commit, or with it as SemVer build
/// metadata, the commit's first twelve digits, and `.dirty` only when the builder said the tree
/// had uncommitted changes: «0.2.0», «0.2.0+3f9c2ab01d4e», «0.2.0+3f9c2ab01d4e.dirty». A `version`
/// that already carries build metadata is left as it is: this adds one, never a second.
pub(crate) fn declared_version(version: &str, head: Option<&Head>, dirty: bool) -> String {
    let Some(head) = head else {
        return version.to_owned();
    };
    if version.contains('+') {
        return version.to_owned();
    }
    let short = &head.commit[..12.min(head.commit.len())];
    format!("{version}+{short}{}", if dirty { ".dirty" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::tests::{disposable, remove_now, resolved_temp_dir};

    const MAIN: &str = "3f9c2ab01d4e5f60718293a4b5c6d7e8f9012345";
    const OTHER: &str = "77AA01C2D3E4F5061728394A5B6C7D8E9F001122";

    fn folder() -> PathBuf {
        let folder = disposable(
            resolved_temp_dir().join(format!("devhq-build-version-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn commit_of(checkout: &Path) -> Option<String> {
        head_of(checkout).map(|head| head.commit)
    }

    #[test]
    fn reads_the_commit_of_head_from_every_layout_of_a_checkout() {
        let root = folder();
        // No repository: a source archive.
        assert_eq!(head_of(&root), None);
        // An ordinary checkout on a branch, its ref loose.
        let plain = root.join("plain");
        write(&plain.join(".git/HEAD"), "ref: refs/heads/main\n");
        write(&plain.join(".git/refs/heads/main"), &format!("{MAIN}\n"));
        let head = head_of(&plain).unwrap();
        assert_eq!(head.commit, MAIN);
        // The files a build must watch: HEAD, and the ref it names.
        assert_eq!(
            head.watch,
            [plain.join(".git/HEAD"), plain.join(".git/refs/heads/main")]
        );
        // A branch in a folder, with its ref packed: comments and peeled tags are skipped.
        let packed = root.join("packed");
        write(
            &packed.join(".git/HEAD"),
            "ref: refs/heads/claude/observe\n",
        );
        write(
            &packed.join(".git/packed-refs"),
            &format!(
                "# pack-refs with: peeled fully-peeled sorted\n{OTHER} refs/heads/claude/observe-more\n{MAIN} refs/heads/claude/observe\n{OTHER} refs/tags/v1\n^{OTHER}\n"
            ),
        );
        assert_eq!(commit_of(&packed).as_deref(), Some(MAIN));
        // Loose and packed: the loose one is the newer.
        write(
            &packed.join(".git/refs/heads/claude/observe"),
            &format!("{OTHER}\n"),
        );
        assert_eq!(
            commit_of(&packed),
            Some(OTHER.to_ascii_lowercase()),
            "the loose ref wins, in lowercase"
        );
        // A detached HEAD.
        let detached = root.join("detached");
        write(&detached.join(".git/HEAD"), &format!("{MAIN}\n"));
        assert_eq!(commit_of(&detached).as_deref(), Some(MAIN));
        // A linked worktree: `.git` is a file that names where its HEAD is, and its branch is in
        // the repository it is linked to, loose or packed.
        let main = root.join("main");
        let worktree = root.join("worktrees").join("feature");
        write(&main.join(".git/HEAD"), "ref: refs/heads/main\n");
        write(&main.join(".git/refs/heads/main"), &format!("{OTHER}\n"));
        write(
            &main.join(".git/worktrees/feature/HEAD"),
            "ref: refs/heads/feature\n",
        );
        write(&main.join(".git/worktrees/feature/commondir"), "../..\n");
        write(&main.join(".git/refs/heads/feature"), &format!("{MAIN}\n"));
        write(
            &worktree.join(".git"),
            &format!(
                "gitdir: {}\n",
                main.join(".git/worktrees/feature").display()
            ),
        );
        let head = head_of(&worktree).unwrap();
        assert_eq!(head.commit, MAIN, "the worktree's branch, not the main one");
        assert!(head.watch.contains(&worktree.join(".git")));
        assert!(head
            .watch
            .contains(&main.join(".git/worktrees/feature/HEAD")));
        std::fs::remove_file(main.join(".git/refs/heads/feature")).unwrap();
        write(
            &main.join(".git/packed-refs"),
            &format!("{MAIN} refs/heads/feature\n"),
        );
        assert_eq!(commit_of(&worktree).as_deref(), Some(MAIN));
        // The pointer may be relative to the checkout, as a submodule's is.
        let relative = root.join("relative");
        write(&relative.join(".git"), "gitdir: ../main/.git\n");
        assert_eq!(commit_of(&relative), Some(OTHER.to_ascii_lowercase()));
        remove_now(root);
    }

    /// No commit is better than a guessed one: whatever this does not understand gives none.
    #[test]
    fn names_no_commit_where_it_cannot_tell() {
        let root = folder();
        let cases: [(&str, &[(&str, &str)]); 9] = [
            ("an empty .git folder", &[(".git/description", "x")]),
            ("a HEAD that names nothing", &[(".git/HEAD", "\n")]),
            (
                "a ref that is nowhere",
                &[(".git/HEAD", "ref: refs/heads/main\n")],
            ),
            (
                "a ref that holds no commit",
                &[
                    (".git/HEAD", "ref: refs/heads/main\n"),
                    (".git/refs/heads/main", "not a commit\n"),
                ],
            ),
            ("a short hash", &[(".git/HEAD", "3f9c2ab\n")]),
            (
                "a ref that climbs out of the repository",
                &[
                    (".git/HEAD", "ref: refs/../../outside\n"),
                    ("outside", MAIN),
                ],
            ),
            ("a ref outside refs/", &[(".git/HEAD", "ref: HEAD\n")]),
            ("a .git file that names no folder", &[(".git", "gitdir:\n")]),
            (
                "a .git file that names a folder that is not there",
                &[(".git", "gitdir: ../nowhere\n")],
            ),
        ];
        for (index, (what, files)) in cases.iter().enumerate() {
            let checkout = root.join(index.to_string()).join("checkout");
            std::fs::create_dir_all(&checkout).unwrap();
            for (name, text) in *files {
                write(&checkout.join(name), text);
            }
            assert_eq!(head_of(&checkout), None, "{what}");
        }
        remove_now(root);
    }

    #[test]
    fn declares_the_commit_as_build_metadata_and_dirty_only_when_told() {
        let head = Head {
            commit: MAIN.to_owned(),
            watch: Vec::new(),
        };
        assert_eq!(declared_version("0.2.0", None, false), "0.2.0");
        // Nothing is said of the tree without a commit to say it of.
        assert_eq!(declared_version("0.2.0", None, true), "0.2.0");
        assert_eq!(
            declared_version("0.2.0", Some(&head), false),
            "0.2.0+3f9c2ab01d4e"
        );
        assert_eq!(
            declared_version("0.2.0", Some(&head), true),
            "0.2.0+3f9c2ab01d4e.dirty"
        );
        // One build metadata, never two.
        assert_eq!(
            declared_version("0.2.0+nightly", Some(&head), true),
            "0.2.0+nightly"
        );
        // What DevHQ compares a declared version with: the commit and the mark do not make it
        // another version.
        for built in ["0.2.0+3f9c2ab01d4e", "0.2.0+3f9c2ab01d4e.dirty"] {
            assert!(crate::installations::readings::same_version("0.2.0", built));
        }
    }
}
