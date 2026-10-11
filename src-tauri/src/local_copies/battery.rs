//! The hostile battery: repositories that name a program everywhere Git lets them, read by the
//! reader as DevHQ reads them. Every program a repository names is a marker that only leaves a
//! mark, and copies of it sit first on the reader's PATH as `git`, `sh`, `gpg`, `ssh`… Reading
//! must leave no mark, change no file (content and modification time), create none, and still read
//! every fact right. The same repository read by the installed `git status` does leave marks: the
//! battery can tell. Any new reader, library or libgit2 version must pass it on Linux and Windows.
//!
//! The hostile config is written last, after the installed `git` built everything, so that the
//! `git` that builds the repositories never runs it.

use super::tests::{folder, git, path_text, test_reader};
use super::{Facts, DEADLINE};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// Every config key `git help --config` lists that names a program Git runs, as that list
/// names it (`<driver>`, `<tool>`… are placeholders), plus the filter keys that only gitattributes
/// documents. `every_program_key_git_knows_is_in_the_battery` holds this list to the installed Git.
const PROGRAM_KEYS: [&str; 54] = [
    "alias.*",
    "alias.*.command",
    "browser.<tool>.cmd",
    "browser.<tool>.path",
    "core.alternateRefsCommand",
    "core.askPass",
    "core.editor",
    "core.fsmonitor",
    "core.gitProxy",
    "core.hooksPath",
    "core.pager",
    "core.sshCommand",
    "credential.<url>.*",
    "credential.helper",
    "diff.<driver>.command",
    "diff.<driver>.textconv",
    "diff.external",
    "difftool.<tool>.cmd",
    "difftool.<tool>.path",
    "filter.<driver>.clean",
    "filter.<driver>.process",
    "filter.<driver>.smudge",
    "gc.recentObjectsHook",
    "gpg.<format>.program",
    "gpg.program",
    "gpg.ssh.defaultKeyCommand",
    "guitool.<name>.cmd",
    "help.browser",
    "hook.<friendly-name>.command",
    "imap.tunnel",
    "instaweb.browser",
    "instaweb.httpd",
    "interactive.diffFilter",
    "man.<tool>.cmd",
    "man.<tool>.path",
    "merge.<driver>.driver",
    "mergetool.<tool>.cmd",
    "mergetool.<tool>.path",
    "pager.<cmd>",
    "remote.<name>.receivepack",
    "remote.<name>.uploadpack",
    "remote.<name>.vcs",
    "sendemail.<identity>.*",
    "sendemail.ccCmd",
    "sendemail.headerCmd",
    "sendemail.sendmailCmd",
    "sendemail.smtpServer",
    "sendemail.toCmd",
    "sequence.editor",
    "submodule.<name>.update",
    "trailer.<key-alias>.cmd",
    "trailer.<key-alias>.command",
    "uploadpack.packObjectsHook",
    "web.browser",
];

/// Keys whose names look like they name a program but do not (a path to read, a proxy URL, a
/// switch), so that the check against `git help --config` only flags real omissions.
const NOT_PROGRAMS: [&str; 19] = [
    "advice.ignoredHook",
    "advice.updateSparsePath",
    "advice.waitingForEditor",
    "color.pager",
    "column.clean",
    "core.quotePath",
    "credential.useHttpPath",
    "diff.<driver>.cachetextconv",
    "fsck.gitmodulesPath",
    "gitcvs.dbDriver",
    "help.htmlPath",
    "http.proxy",
    "http.sslCAPath",
    "include.path",
    "includeIf.<condition>.path",
    "instaweb.modulePath",
    "receive.fsck.gitmodulesPath",
    "remote.<name>.proxy",
    "sendemail.smtpSSLCertPath",
];

/// Hooks Git runs from `.git/hooks` or `core.hooksPath`, by name.
const HOOKS: [&str; 28] = [
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// Programs Git or a helper would look for on the PATH.
const ON_PATH: [&str; 12] = [
    "git",
    "sh",
    "bash",
    "gpg",
    "gpgsm",
    "ssh",
    "ssh-keygen",
    "git-lfs",
    "git-remote-x",
    "git-fsmonitor--daemon",
    "less",
    "vi",
];

/// The marker's source: it appends its own file name and arguments to `marks.txt`, two folders
/// up from itself, and fails.
const MARKER: &str = r#"
use std::io::Write;
fn main() {
    let me = std::env::current_exe().unwrap_or_default();
    let name = me.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    if let Some(root) = me.parent().and_then(|bin| bin.parent()) {
        if let Ok(mut marks) = std::fs::OpenOptions::new().create(true).append(true).open(root.join("marks.txt")) {
            let args: Vec<String> = std::env::args().skip(1).collect();
            let _ = writeln!(marks, "{name} {}", args.join(" "));
        }
    }
    std::process::exit(1);
}
"#;

/// The marker program, compiled into `dir` with the toolchain that builds the tests: a real
/// executable, which Windows needs, and the same on every system. It lives in the battery's own
/// folder, so that nothing of it outlives the test.
fn compile_marker(dir: &Path) -> PathBuf {
    std::fs::create_dir(dir).unwrap();
    let source = dir.join("marker.rs");
    std::fs::write(&source, MARKER).unwrap();
    let program = dir.join(format!("marker{}", std::env::consts::EXE_SUFFIX));
    // Cargo names itself in CARGO; rustc sits next to it.
    let rustc = std::env::var_os("CARGO")
        .map(|cargo| {
            Path::new(&cargo).with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX))
        })
        .filter(|rustc| rustc.is_file())
        .unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .args(["--edition", "2021", "-C", "debuginfo=0", "-o"])
        .arg(&program)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    program
}

/// The battery's folders: `marker` (the compiled marker), `bin` (the PATH copies), `keys` (one
/// marker per key), `hooks` (for `core.hooksPath`) and `copies` (the repositories), and
/// `marks.txt` once anything ran.
struct Battery {
    root: PathBuf,
    marker: PathBuf,
}

impl Battery {
    fn new() -> Self {
        let root = folder("battery");
        for dir in ["bin", "keys", "hooks", "copies"] {
            std::fs::create_dir(root.join(dir)).unwrap();
        }
        let marker = compile_marker(&root.join("marker"));
        Self { root, marker }
    }

    fn copies(&self) -> PathBuf {
        self.root.join("copies")
    }

    /// A marker at `dir/name`, linked rather than copied: one file on disk however many names.
    fn plant(&self, dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        if std::fs::hard_link(&self.marker, &path).is_err() {
            std::fs::copy(&self.marker, &path).unwrap();
        }
        // A hook is found by its bare name; on Windows, plant that name too.
        if cfg!(windows) && dir.ends_with("hooks") {
            std::fs::copy(&self.marker, dir.join(name)).unwrap();
        }
        path
    }

    /// The marker for a config key, as Git config reads a path: forward slashes, quoted.
    fn key(&self, key: &str) -> String {
        let name: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let path = self.plant(&self.root.join("keys"), &format!("key-{name}"));
        format!("\"{}\"", path_text(&path).replace('\\', "/"))
    }

    fn marks(&self) -> String {
        std::fs::read_to_string(self.root.join("marks.txt")).unwrap_or_default()
    }

    /// The reader's PATH: the markers first, then the machine's own.
    fn path(&self) -> OsString {
        let mut paths = vec![self.root.join("bin")];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(paths).unwrap()
    }
}

/// Hostile config naming a marker for every key, for a repository whose remotes and submodule
/// are `origin` and `sub`.
fn hostile_config(battery: &Battery) -> String {
    let k = |key: &str| battery.key(key);
    let root = path_text(&battery.root).replace('\\', "/");
    let copies = path_text(&battery.copies()).replace('\\', "/");
    format!(
        r#"
[core]
	fsmonitor = {fsmonitor}
	fsmonitorHookVersion = 2
	hooksPath = "{root}/hooks"
	sshCommand = {ssh}
	gitProxy = {proxy}
	editor = {editor}
	pager = {pager}
	askPass = {askpass}
	alternateRefsCommand = {alternate}
	untrackedCache = true
[feature]
	manyFiles = true
[alias]
	x = "!{alias}"
[alias "y"]
	command = {alias_command}
[browser "x"]
	cmd = {browser_cmd}
	path = {browser_path}
[credential]
	helper = {credential}
[credential "https://example.invalid"]
	helper = {credential_url}
[diff]
	external = {diff_external}
[diff "x"]
	command = {diff_command}
	textconv = {textconv}
[difftool "x"]
	cmd = {difftool_cmd}
	path = {difftool_path}
[filter "x"]
	clean = {clean}
	smudge = {smudge}
	process = {process}
	required = true
[filter "lfs"]
	clean = {lfs_clean}
	smudge = {lfs_smudge}
	process = {lfs_process}
	required = true
[gc]
	recentObjectsHook = {recent}
[gpg]
	program = {gpg}
[gpg "openpgp"]
	program = {openpgp}
[gpg "x509"]
	program = {x509}
[gpg "ssh"]
	program = {gpg_ssh}
	defaultKeyCommand = {default_key}
[guitool "x"]
	cmd = {guitool}
[help]
	browser = {help_browser}
[imap]
	tunnel = {imap_tunnel}
[hook "x"]
	command = {hook_command}
	event = pre-commit
	event = post-index-change
	event = reference-transaction
[instaweb]
	browser = {instaweb_browser}
	httpd = {httpd}
[interactive]
	diffFilter = {diff_filter}
[log]
	showSignature = true
[man "x"]
	cmd = {man_cmd}
	path = {man_path}
[merge "x"]
	driver = {merge_driver}
[mergetool "x"]
	cmd = {mergetool_cmd}
	path = {mergetool_path}
[pager]
	status = {pager_cmd}
	log = {pager_cmd}
[remote "origin"]
	receivepack = {receivepack}
	uploadpack = {uploadpack}
[remote "evil"]
	url = "ext::{ext} %S"
	vcs = x
	receivepack = {receivepack}
	uploadpack = {uploadpack}
[remote "helper"]
	url = x::https://example.invalid/shop
[protocol "ext"]
	allow = always
[sendemail]
	ccCmd = {cc}
	headerCmd = {header}
	sendmailCmd = {sendmail}
	smtpServer = {smtp}
	toCmd = {to}
[sendemail "x"]
	toCmd = {identity}
[sequence]
	editor = {sequence}
[submodule "sub"]
	update = "!{submodule}"
[trailer "x"]
	cmd = {trailer_cmd}
	command = {trailer_command}
[uploadpack]
	packObjectsHook = {pack_objects}
[web]
	browser = {web}
[include]
	path = "{root}/included.conf"
[includeIf "gitdir:{copies}/"]
	path = "{root}/included-if.conf"
"#,
        fsmonitor = k("core.fsmonitor"),
        ssh = k("core.sshCommand"),
        proxy = k("core.gitProxy"),
        editor = k("core.editor"),
        pager = k("core.pager"),
        askpass = k("core.askPass"),
        alternate = k("core.alternateRefsCommand"),
        alias = k("alias.*").trim_matches('"').to_owned(),
        alias_command = k("alias.*.command"),
        browser_cmd = k("browser.<tool>.cmd"),
        browser_path = k("browser.<tool>.path"),
        credential = k("credential.helper"),
        credential_url = k("credential.<url>.*"),
        diff_external = k("diff.external"),
        diff_command = k("diff.<driver>.command"),
        textconv = k("diff.<driver>.textconv"),
        difftool_cmd = k("difftool.<tool>.cmd"),
        difftool_path = k("difftool.<tool>.path"),
        clean = k("filter.<driver>.clean"),
        smudge = k("filter.<driver>.smudge"),
        process = k("filter.<driver>.process"),
        lfs_clean = k("filter.lfs.clean"),
        lfs_smudge = k("filter.lfs.smudge"),
        lfs_process = k("filter.lfs.process"),
        recent = k("gc.recentObjectsHook"),
        gpg = k("gpg.program"),
        openpgp = k("gpg.<format>.program"),
        x509 = k("gpg.x509.program"),
        gpg_ssh = k("gpg.ssh.program"),
        default_key = k("gpg.ssh.defaultKeyCommand"),
        guitool = k("guitool.<name>.cmd"),
        help_browser = k("help.browser"),
        imap_tunnel = k("imap.tunnel"),
        hook_command = k("hook.<friendly-name>.command"),
        instaweb_browser = k("instaweb.browser"),
        httpd = k("instaweb.httpd"),
        diff_filter = k("interactive.diffFilter"),
        man_cmd = k("man.<tool>.cmd"),
        man_path = k("man.<tool>.path"),
        merge_driver = k("merge.<driver>.driver"),
        mergetool_cmd = k("mergetool.<tool>.cmd"),
        mergetool_path = k("mergetool.<tool>.path"),
        pager_cmd = k("pager.<cmd>"),
        receivepack = k("remote.<name>.receivepack"),
        uploadpack = k("remote.<name>.uploadpack"),
        ext = k("remote ext").trim_matches('"').to_owned(),
        cc = k("sendemail.ccCmd"),
        header = k("sendemail.headerCmd"),
        sendmail = k("sendemail.sendmailCmd"),
        smtp = k("sendemail.smtpServer"),
        to = k("sendemail.toCmd"),
        identity = k("sendemail.<identity>.*"),
        sequence = k("sequence.editor"),
        submodule = k("submodule.<name>.update").trim_matches('"').to_owned(),
        trailer_cmd = k("trailer.<key-alias>.cmd"),
        trailer_command = k("trailer.<key-alias>.command"),
        pack_objects = k("uploadpack.packObjectsHook"),
        web = k("web.browser"),
    )
}

/// Appends `text` to the file at `path`, creating it.
fn append(path: &Path, text: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

/// Gives the file at `path` a modification time two minutes from now, so that its stat no longer
/// matches the index and its content has to be compared.
fn touch(path: &Path) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(120))
        .unwrap();
}

/// What `git` itself says about a repository, before anything hostile is in it.
struct Expected {
    head: String,
    last_commit_at: i64,
}

/// The hostile copy and everything around it, built by the installed `git`, then made hostile.
/// Returns the copies to read and what the main one must read as.
fn build(battery: &Battery) -> (Vec<PathBuf>, Expected) {
    let copies = battery.copies();
    let origin = copies.join("origin.git");
    std::fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    let sub_origin = copies.join("sub-origin");
    std::fs::create_dir(&sub_origin).unwrap();
    git(&sub_origin, &["init", "-q", "-b", "main"]);
    std::fs::write(sub_origin.join("lib.txt"), "lib\n").unwrap();
    git(&sub_origin, &["add", "-A"]);
    git(&sub_origin, &["commit", "-q", "-m", "lib"]);

    let shop = copies.join("shop");
    std::fs::create_dir(&shop).unwrap();
    git(&shop, &["init", "-q", "-b", "main"]);
    git(&shop, &["config", "extensions.worktreeConfig", "true"]);
    std::fs::write(shop.join("README.md"), "hello\n").unwrap();
    std::fs::write(shop.join("notes.txt"), "notes\n").unwrap();
    std::fs::write(shop.join(".gitignore"), "*.log\n").unwrap();
    git(&shop, &["add", "-A"]);
    git(&shop, &["commit", "-q", "-m", "first"]);
    git(
        &shop,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &path_text(&sub_origin),
            "sub",
        ],
    );
    git(&shop, &["commit", "-q", "-m", "submodule"]);
    git(&shop, &["remote", "add", "origin", &path_text(&origin)]);
    git(&shop, &["push", "-q", "origin", "main"]);
    // Unpublished elsewhere: a branch never pushed, and a signed commit on another.
    git(&shop, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(shop.join("feature.txt"), "f\n").unwrap();
    git(&shop, &["add", "-A"]);
    git(&shop, &["commit", "-q", "-m", "feature"]);
    git(&shop, &["checkout", "-q", "main"]);
    let tree = git(&shop, &["rev-parse", "HEAD^{tree}"]);
    let parent = git(&shop, &["rev-parse", "HEAD"]);
    let signed = format!(
        "tree {}\nparent {}\nauthor DevHQ test <test@devhq.invalid> 1700000000 +0000\ncommitter DevHQ test <test@devhq.invalid> 1700000000 +0000\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n wsBcBAABCAAQBQJlVPPPCRAAAAAAAAAAAA==\n -----END PGP SIGNATURE-----\n\nsigned\n",
        tree.trim(),
        parent.trim()
    );
    let object_file = battery.root.join("signed-commit");
    std::fs::write(&object_file, signed).unwrap();
    let oid = git(
        &shop,
        &[
            "hash-object",
            "-t",
            "commit",
            "-w",
            &path_text(&object_file),
        ],
    );
    git(&shop, &["update-ref", "refs/heads/signed", oid.trim()]);
    // A stash, and a linked worktree on a branch of its own.
    std::fs::write(shop.join("README.md"), "stashed\n").unwrap();
    git(&shop, &["stash", "-q"]);
    let linked = copies.join("shop-linked");
    git(
        &shop,
        &["worktree", "add", "-q", "-b", "linked", &path_text(&linked)],
    );
    // Unpublished on HEAD: one commit after the push. It also adds two files under filter
    // drivers, stored as their clean filters made them, so that what Git keeps is not what the
    // working tree holds. Last of all, so that no later `git` looks at them again.
    std::fs::write(shop.join("CHANGELOG.md"), "1.0\n").unwrap();
    std::fs::write(shop.join("data.f"), "a\n").unwrap();
    std::fs::write(shop.join("big.lfs"), "b\n").unwrap();
    std::fs::write(
        shop.join(".gitattributes"),
        "*.f filter=x diff=x merge=x\n*.lfs filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    git(
        &shop,
        &[
            "-c",
            "filter.x.clean=sed s/^/X/",
            "-c",
            "filter.lfs.clean=sed s/^/P/",
            "add",
            "CHANGELOG.md",
            "data.f",
            "big.lfs",
            ".gitattributes",
        ],
    );
    git(&shop, &["commit", "-q", "-m", "changelog"]);
    // A staged change and an unstaged one.
    std::fs::write(shop.join("notes.txt"), "more notes\n").unwrap();
    git(&shop, &["add", "notes.txt"]);
    std::fs::write(shop.join("README.md"), "hello, world\n").unwrap();
    // New entries: a file, a folder (once), and the nested repository (once); and an ignored one.
    std::fs::write(shop.join("new.txt"), "n\n").unwrap();
    std::fs::create_dir(shop.join("newdir")).unwrap();
    std::fs::write(shop.join("newdir").join("a.txt"), "a\n").unwrap();
    std::fs::write(shop.join("newdir").join("b.txt"), "b\n").unwrap();
    std::fs::write(shop.join("debug.log"), "log\n").unwrap();
    // An unregistered repository inside the copy.
    let nested = shop.join("nested");
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-q", "-b", "main"]);
    std::fs::write(nested.join("x.txt"), "x\n").unwrap();
    git(&nested, &["add", "-A"]);
    git(&nested, &["commit", "-q", "-m", "nested"]);
    // A copy whose fsmonitor is Git's own daemon, which `git` starts as `git fsmonitor--daemon`.
    let daemon = copies.join("daemon");
    std::fs::create_dir(&daemon).unwrap();
    git(&daemon, &["init", "-q", "-b", "main"]);
    std::fs::write(daemon.join("a.txt"), "a\n").unwrap();
    git(&daemon, &["add", "-A"]);
    git(&daemon, &["commit", "-q", "-m", "a"]);

    let expected = Expected {
        head: git(&shop, &["rev-parse", "HEAD"]).trim().to_owned(),
        last_commit_at: git(
            &shop,
            &[
                "for-each-ref",
                "--sort=-committerdate",
                "--count=1",
                "--format=%(committerdate:unix)",
                "refs/heads",
            ],
        )
        .trim()
        .parse::<i64>()
        .unwrap()
            * 1000,
    };
    // Under filters DevHQ does not run, the working tree's text differs from what Git keeps.
    touch(&shop.join("data.f"));
    touch(&shop.join("big.lfs"));

    // Only now is anything hostile written, so that the `git` above never ran it.
    let config = hostile_config(battery);
    let shop_git = shop.join(".git");
    let sub_git = shop_git.join("modules").join("sub");
    for config_file in [
        shop_git.join("config"),
        sub_git.join("config"),
        nested.join(".git").join("config"),
    ] {
        append(&config_file, &config);
    }
    append(
        &daemon.join(".git").join("config"),
        "[core]\n\tfsmonitor = true\n\tuntrackedCache = true\n",
    );
    append(
        &shop_git.join("config.worktree"),
        &format!("[core]\n\tfsmonitor = {}\n", battery.key("config.worktree")),
    );
    append(
        &shop_git
            .join("worktrees")
            .join("shop-linked")
            .join("config.worktree"),
        &format!(
            "[core]\n\tfsmonitor = {}\n",
            battery.key("linked config.worktree")
        ),
    );
    append(
        &battery.root.join("included.conf"),
        &format!(
            "[core]\n\tfsmonitor = {}\n[filter \"x\"]\n\tclean = {}\n",
            battery.key("include.path fsmonitor"),
            battery.key("include.path clean")
        ),
    );
    append(
        &battery.root.join("included-if.conf"),
        &format!(
            "[core]\n\tpager = {}\n[filter \"x\"]\n\tprocess = {}\n",
            battery.key("includeIf pager"),
            battery.key("includeIf process")
        ),
    );
    for hooks in [
        battery.root.join("hooks"),
        shop_git.join("hooks"),
        sub_git.join("hooks"),
    ] {
        std::fs::create_dir_all(&hooks).unwrap();
        for hook in HOOKS {
            battery.plant(&hooks, hook);
        }
    }
    for name in ON_PATH {
        battery.plant(&battery.root.join("bin"), name);
    }
    let sub = shop.join("sub");
    (vec![shop, linked, sub, nested, daemon], expected)
}

/// Every file and folder under `root`: its length, the SHA-256 of its content and when it was last
/// modified.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, Vec<u8>, Option<SystemTime>)> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            let modified = metadata.modified().ok();
            if metadata.is_dir() {
                found.insert(path.clone(), (0, Vec::new(), modified));
                pending.push(path);
            } else {
                let hash = Sha256::digest(std::fs::read(&path).unwrap()).to_vec();
                found.insert(path, (metadata.len(), hash, modified));
            }
        }
    }
    found
}

#[test]
fn reading_a_hostile_copy_runs_nothing_writes_nothing_and_reads_it_right() {
    let battery = Battery::new();
    let (copies, expected) = build(&battery);
    let before = snapshot(&battery.copies());
    let path = battery.path();
    // Once without the system's barrier, to show that libgit2 itself starts nothing; once as
    // DevHQ reads, confined.
    for mode in ["unconfined", "confined"] {
        let reader = test_reader(mode, &[("PATH", &path)]);
        let mut readings: Vec<Facts> = Vec::new();
        for copy in &copies {
            let read = reader
                .read(&path_text(copy), DEADLINE)
                .unwrap_or_else(|error| panic!("{mode} {}: {}", copy.display(), error.message));
            assert_eq!(
                read.refs.outcome,
                "read",
                "{mode} {}: {read:?}",
                copy.display()
            );
            readings.push(read);
        }
        assert_eq!(battery.marks(), "", "{mode}: a program ran");
        let after = snapshot(&battery.copies());
        let appeared: Vec<_> = after
            .keys()
            .filter(|file| !before.contains_key(*file))
            .collect();
        assert!(appeared.is_empty(), "{mode}: {appeared:?} appeared");
        for (file, state) in &before {
            assert_eq!(
                after.get(file),
                Some(state),
                "{mode}: {} changed",
                file.display()
            );
        }

        let shop = &readings[0];
        let refs = &shop.refs;
        assert_eq!(refs.branch.as_deref(), Some("main"));
        assert_eq!(refs.head.as_deref(), Some(expected.head.as_str()));
        assert_eq!(refs.last_commit_at, Some(expected.last_commit_at));
        assert_eq!(refs.unpublished_on_head, Some(1), "the changelog commit");
        assert_eq!(
            refs.unpublished_elsewhere,
            Some(2),
            "feature and the signed commit"
        );
        assert_eq!(refs.stashes, Some(1));
        assert!(refs.synced_at.is_some(), "the push updated origin/main");
        assert_eq!(refs.operation, None);
        let remotes: Vec<(&str, Option<&str>)> = refs
            .remotes
            .as_ref()
            .unwrap()
            .iter()
            .map(|remote| (remote.name.as_str(), remote.url.as_deref()))
            .collect();
        let origin = path_text(&battery.copies().join("origin.git"));
        // By name, as libgit2 lists them; a command line or a helper's address is never kept.
        assert_eq!(
            remotes,
            [
                ("evil", None),
                ("helper", None),
                ("origin", Some(origin.as_str()))
            ]
        );
        let worktree = shop.worktree.as_ref().unwrap();
        assert_eq!(
            (
                worktree.changed,
                worktree.untracked,
                worktree.unverifiable,
                worktree.unknown.as_deref()
            ),
            (Some(2), Some(3), Some(2), None),
            "README and notes; new.txt, newdir and nested; data.f and big.lfs"
        );
        // The linked worktree reads as its own copy, on its own branch.
        assert_eq!(readings[1].refs.branch.as_deref(), Some("linked"));
    }

    // The battery can tell: the installed `git` runs what the same repository names.
    let _ = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&copies[0])
        .env("PATH", &path)
        .output()
        .unwrap();
    assert_ne!(
        battery.marks(),
        "",
        "git status ran nothing, so the battery proves nothing"
    );
    std::fs::remove_dir_all(&battery.root).unwrap();
}

/// The keys `git help --config` lists whose name says they may name a program. A family of
/// settings (`advice.*`, `bundle.<id>.*`…) is left to the keys it names one by one; the families that
/// can name programs (aliases, credential helpers, sendemail identities) are in the battery.
fn program_like_keys() -> Vec<String> {
    let listed = Command::new("git")
        .args(["help", "--config"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    let endings = [
        "command",
        "cmd",
        "program",
        "helper",
        "hook",
        "editor",
        "pager",
        "driver",
        "textconv",
        "external",
        "smudge",
        "clean",
        "process",
        "proxy",
        "askpass",
        "browser",
        "receivepack",
        "uploadpack",
        "vcs",
        "httpd",
        "difffilter",
        "path",
        "fsmonitor",
    ];
    String::from_utf8(listed.stdout)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|key| {
            let last = key.rsplit('.').next().unwrap_or(key).to_ascii_lowercase();
            endings.iter().any(|ending| last.ends_with(ending)) || key.starts_with("pager.")
        })
        .map(str::to_owned)
        .collect()
}

/// A key as Git versions agree on it: its placeholders unnamed (`<token>` became `<key-alias>`)
/// and its case ignored.
fn shape(key: &str) -> String {
    let mut shaped = String::new();
    let mut inside = false;
    for c in key.chars() {
        match c {
            '<' => {
                inside = true;
                shaped.push('<');
            }
            '>' => {
                inside = false;
                shaped.push('>');
            }
            _ if !inside => shaped.push(c.to_ascii_lowercase()),
            _ => {}
        }
    }
    shaped
}

#[test]
fn every_program_key_git_knows_is_in_the_battery() {
    let unknown: Vec<String> = program_like_keys()
        .into_iter()
        .filter(|key| {
            let key = shape(key);
            !PROGRAM_KEYS
                .iter()
                .chain(&NOT_PROGRAMS)
                .any(|known| shape(known) == key)
        })
        .collect();
    assert!(
        unknown.is_empty(),
        "This Git knows config keys that may name a program and that the hostile battery does not set: {unknown:?}. Add each to PROGRAM_KEYS (and to hostile_config) or, if it names no program, to NOT_PROGRAMS."
    );
}
