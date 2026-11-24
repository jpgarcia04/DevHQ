# DevHQ

Native desktop app (Tauri 2): Rust + SQLite in `src-tauri/`, React + TypeScript + Vite +
Tailwind 4 in `src/`. `docs/especificacion.md` (in Spanish) is the specification: read it before
changing behaviour, and update it in the same commit as the change. It lived in `README.md` until
November 2026, so wherever code, comments, `docs/`, tests or the app's messages say «the README»
(often with a «section»), they mean `docs/especificacion.md`; new text names that file. `README.md`
is now only the public presentation of the project (what it is, screenshots of the demo, stack, how
to try it, links to the specification): keep it short and never put behaviour there. `AGENTS.md`
sends Codex and other agents here: keep every rule in this file and only pointers there.

## Product rules

- Every view answers «¿Qué necesita mi atención?». A feature that only saves typing a
  simple command does not belong here.
- Nothing empty, missing, unknown or never observed is presented as healthy or complete.
  When a list is cut (history window, paging), the UI says where and why.
- History is never deleted or rewritten: observations are immutable, subjects with history
  cannot be deleted, a schema change keeps every row.
- DevHQ observes; it does not operate systems or run in the background. It holds one credential,
  a read-only fine-grained GitHub token, and only in the system's credential store (see
  «Credentials»). It reads only what the inventory names, when asked or, for this workstation's
  local copies and installations, once when its window opens (specification «Trabajo pendiente»),
  and never searches a disk or a network for things to add. Observing never gives the source a way to run code or
  change anything: a reader never runs a program the source names and never writes to it, and every
  declared path, and what it holds, is untrusted data.
- Provenance (`source`), observed and recorded times, and validity are always visible.
- `docs/vision.md` (in Spanish) holds the long-term direction and the questions to ask of any
  new feature or architectural change; read it before proposing either. It guides and
  authorizes nothing: the specification stays the specification, and the rules above hold until a
  change that must break one rewrites that rule, here and in the specification, in the same commit.

## What stays out of the repository

The repository is public and holds only the product: its code, its tests, its documentation and a
demo of invented systems. The person's own use of DevHQ never enters it, in any commit:

- Their real inventory and everything derived from it: the systems they register (applications,
  resources, components, paths, repositories, URLs), observations, incidents, events, readings,
  backups, copies, exports, answers read from it, screenshots of their window, and hashes or counts
  that identify any of these. It lives in DevHQ's data folder (see `identity`), never in a checkout.
- Their notes, validation records and reports, and the details of their machines: hardware, user
  and folder names, machine fingerprints, which workstation is which.
- Tests, fixtures, contracts, the demo (`src/data/mock/`), the preview (`src/preview/`) and the
  documentation use only invented systems, people and paths (`*.example`, `*.invalid`, `Ana Ruiz`):
  never a real record copied in, not even renamed.
- Before every commit, read what it adds (`git diff --cached`). `.gitignore` refuses database files,
  `this-workstation.json` and `notas/`, but it cannot recognise a real name written in a source file.

## Checks

Before every commit, run the checks that what it touches can break, and look at each exit code.
Never pipe a check through `tail` or `head`: the pipeline's exit code is the last command's, which
hides a failure. The full set costs minutes and a native build; spend it where it can fail:

- Only documentation (`*.md`, `docs/`): `npx prettier --check .`. It is the only check such a
  change gets: `.github/workflows/ci.yml` skips a pull request or a push to `main` that touches
  nothing else. A test that reads a Markdown file takes that file out of its `paths-ignore` in the
  same change.
- Only when a workflow runs (its `on:` triggers, `paths-ignore`): prettier. No local check runs a
  trigger; the runs the pull request starts or skips are its test.
- Only the version number in its six files (see «Version»): prettier,
  `npx vitest run src/shared/version.test.ts` and
  `cargo metadata --manifest-path src-tauri/Cargo.toml --locked --format-version 1`, which refuses
  a `Cargo.lock` that no longer matches its manifest without compiling anything.
- Only the window (`src/`, `e2e/` outside `e2e/native/`, `index.html`, `preview.html`): prettier,
  eslint, `tsc -b`, vitest, `npm run build` and `npm run e2e`. Not when it changes what crosses IPC
  (`inventoryApi`, the contract tests), a rule `contract/attention/` holds, or a label, button or
  heading the native walk finds: those are the next group.
- Anything else (`src-tauri/`, `contract/`, `e2e/native/`, dependencies, `.github/`, tool
  configuration) and a mix you are unsure of: the full set below. In a pull request with several
  commits, the full set runs on its last commit; an earlier one may run only the checks of the
  side it changed (a Rust change: `cargo fmt`, `cargo clippy` and `cargo test`).

`npm run e2e:native` needs a driver that matches the installed WebView2 (see «Native walk»); when
it cannot run here, say so, and the Windows jobs run it.

```sh
npx prettier --check .
npx eslint .
npx tsc -b
npx vitest run
npm run build
npm run e2e
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --lib --locked
npm run e2e:native
```

Confirm that each new test fails without the fix (temporary mutation). Never restore a
file that has uncommitted changes with `git checkout`.

## Temporary resources

Whatever a session, a test, a script or a validation creates for its own use (a synthetic
inventory, a temporary folder, a helper process, a transient log, a disposable copy) is removed by
what created it as soon as it is no longer needed. Keeping one needs an explicit reason, written
where it stays (a note beside it, the report that uses it): what it is, why it stays, when it can go.

- A test removes what it creates when it ends, whether it passes or fails an assertion, and kills
  and waits for every process it starts, on drop too (on Windows an orphan inherits the output
  pipe and keeps `cargo test` waiting). A failing test keeps its files only when asked to, and
  then says where they are. A Rust test passes each temporary path through
  `inventory::tests::disposable`, which removes it when the test's thread ends, failed or not
  (`DEVHQ_KEEP_TEST_FILES` keeps it); the Windows workflow checks that a passing run leaves nothing.
  A test that removes something itself uses `inventory::tests::remove_now`, never
  `std::fs::remove_dir_all(…).unwrap()`: Windows refuses a removal while anything holds a file,
  Defender's real-time protection included, and `remove_now` tries again for up to 10 s.
- What a forced stop leaves (a killed run, an abort, a crash) cannot be removed by the process
  that made it: a killed `cargo test` leaves its `devhq-<name>-<uuid>` entries in the temporary
  folder. Remove it from outside only by the exact names its creator gives (prefix and
  unique id), only once nothing uses it and no run that could own it is in progress, and list it
  before removing it; where that cannot be done safely, write the limitation down instead.
- Never clean by a broad pattern (`devhq-*`, a whole `%TEMP%`), never remove what someone else
  made (another agent's folder, the person's data, a backup or an archive), and never remove what
  explains a failure before someone has looked at it.
- A copy derived from the person's data (a validation's snapshot or work copy, the answers read
  from it) is never a disposable any agent may remove: the person authorizes beforehand that it is
  made, where it goes and how and when it is removed, and no agent removes it, moves it or keeps
  it longer on its own.
- Build caches are not temporary: Cargo's `target/` or build directory, `node_modules` and the
  downloaded browsers and drivers are reused from run to run and never removed after a test. They
  follow a retention policy of their own, preferably on a data drive rather than the system one
  (on the Windows laptop, D:), and are removed on purpose: a toolchain change, space running out.
  A session never changes the person's global configuration (`~/.cargo/config.toml` and the like)
  on its own, since every other build and agent shares it; a variable or `--config` for one
  command does not.

## Cloud sessions

- `.claude/hooks/session-start.sh` installs WebKitGTK, WebKitWebDriver, Xvfb and
  tauri-driver, gives the container an `/etc/machine-id` if it has none (DevHQ needs one to
  choose this computer, and the native walk chooses it), upgrades the container's npm to 11,
  runs `npm ci`, fetches crates and exports `DEVHQ_CHROMIUM`. It only runs when the branch the session starts on contains it. If it did
  not run, run it by hand:
  `CLAUDE_CODE_REMOTE=true CLAUDE_PROJECT_DIR=$PWD bash .claude/hooks/session-start.sh`.
- `npm run e2e` never downloads a browser. Without `DEVHQ_CHROMIUM` it looks for the system
  Chrome; in the container use
  `DEVHQ_CHROMIUM=$(ls -d /opt/pw-browsers/chromium-*/chrome-linux/chrome | sort -V | tail -1)`.
- The native window runs here: `npm run e2e:native` drives the real app with WebKitGTK under
  its own Xvfb. WebView2, SChannel and the NSIS installer cannot: say what remains to be
  checked on Windows instead of claiming it works.

## Dependencies

The lockfile is written by npm 11 (desktop and CI). npm 10 drops the `libc` fields of
platform-specific packages whenever it rewrites `package-lock.json`. Before adding, removing
or updating a dependency, check that `npm --version` is 11 or later, use a full
`npm install <pkg>` (not `--package-lock-only`), and check with `git diff` that the
lockfile only gained the new entries; then confirm with `npm ci`.

## Preview

`preview.html` (development only) runs Mi inventario over a simulated native process
(`@tauri-apps/api/mocks`) and sample data in `src/preview/`. Its read commands follow the
Rust windowing and paging rules (`src/preview/backend.test.ts`); every write is refused.
`?scenario=empty`, `?scenario=unavailable` and `?scenario=outdated` show an empty, an unreadable and
an older-schema inventory.
To look at the UI: `npx vite --host 127.0.0.1 --port 1420`, then open
`http://127.0.0.1:1420/preview.html#/...` with the global Playwright (`npm root -g`).

## Native walk

`e2e/native/` drives the real app with a WebDriver client of its own over `fetch` (no
dependency): through tauri-driver and WebKitWebDriver on Linux; on Windows msedgedriver attaches
to the DevTools port that only the build from `e2e/native/build.ts` opens, because tauri-driver
cannot launch a Tauri app on WebView2 150 or later. It finds elements by XPath on visible text and labels, so renaming a
label, a button or a heading it uses means updating `main.ts`. It never touches a real
inventory: it drives only an executable the installer did not pack (development data, see
below); on Linux HOME and every XDG directory point into a temporary folder; on Windows it refuses
to run while `%USERPROFILE%\DevHQ-desarrollo`, `%LOCALAPPDATA%\com.devhq.desktop.dev` (the WebView2
profile) or `%APPDATA%\com.devhq.desktop.dev` (the previous location) exists, and removes what it
created. Its agent gate starts without `--database`, as Claude Code starts it, and must open the
window's file. It closes the window
through the system (X11 `WM_DELETE_WINDOW`, Win32 `WM_CLOSE`), never through WebDriver, whose
Close Window only destroys the WebView. Screenshots of each step land in `test-results/native/`.

## Development data

`src-tauri/src/identity.rs` names the data folder for the window and the gate alike, in the home,
never in AppData: only the executable the NSIS installer packed (Tauri's bundle marker) keeps
`%USERPROFILE%\DevHQ`, the person's inventory; `tauri dev`, debug builds, `target/release` and the
tests keep `%USERPROFILE%\DevHQ-desarrollo` (`~/DevHQ-desarrollo` on Linux), and the window title says
«development data». The folder holds the inventory, `backups/`, the window lock and
`this-workstation.json`; only the WebView2 profile stays in Tauri's local folder, named after the
identifier (`com.devhq.desktop` or `.dev`). The marker says how the file was built, not where it is
installed, and it does not stop a link or `--database`: never point a development build at the
person's inventory, and never run anything that writes `%USERPROFILE%\DevHQ` during development.
Anything that resolves a data path goes through `identity`. `identity::previous_database` names where
DevHQ kept the inventory in AppData before; `src-tauri/src/location.rs` only looks at it, never opens,
moves or deletes it: the window starts no inventory while it holds one and the new folder none
(`move_pending`), and the person moves it once, by copying and verifying (specification «Trasladar el
inventario fuera de AppData»). The uninstaller never names the home folder, and
`src-tauri/windows/installer-hooks.nsh` keeps «Delete the application data» from deleting an
inventory or backups still in the previous location.

## Local Windows sessions

The Claude and Codex desktop apps are packaged (MSIX), and Windows redirects AppData for every
process they start, even one without the package's identity (Claude Code's own processes and their
shells answer 15700 to `GetCurrentPackageFullName` and are still redirected): they read and write the
package's copy (`%LOCALAPPDATA%\Packages\<package>\LocalCache\...`), merged with the real AppData,
while File Explorer and the installed DevHQ see only the real one. The home is not redirected, which
is why DevHQ's data lives there: the window and a gate started from those apps open the same file.
Never take a missing package identity as proof that a shell sees the real AppData; for real-data
steps require a parent `explorer` (Win+R) or run outside the package through WMI
(`Invoke-CimMethod Win32_Process -MethodName Create`). `storedAt` says where a file really is, and
agents cannot write through the gate while it is not null.

## Windows workflow

`.github/workflows/windows.yml` runs on demand and on pull requests, but its Windows jobs only run
when a push changed `src-tauri/`, `contract/`, `e2e/native/`, the workflow, `.github/windows/` or
`package-lock.json` since the newest commit of the branch on which every Windows job passed
(`.github/windows/needs-windows.sh`, which a Linux job runs first). A push that only touches `src/`
or documentation skips them. A run costs about 22 billable minutes with a warm cache and 48 cold:
group the changes it tests, push rarely, and do not push while a run whose result you need is in
progress, because a new push cancels it.

A merge does not wait for these jobs once the local checks passed, unless the change touches what
behaves differently on Windows: code under `cfg(windows)`, data paths and where the system keeps a
file (`identity`, `location`, `inventory::stored_at`), `this_computer`, the readers' confinement
(`local_copies::confine`) and their count of processes, the window lock, the NSIS installer and its
hooks, or a test about Defender, NTFS or SChannel. A merged change is never tested on Windows again:
the workflow does not run on `main`, and a later pull request compares from where it left `main`. So
never cancel or supersede the run of a pull request merged without it; read its result once it
finishes, and a failure is the next fix. If it did not finish, run the workflow by hand on `main`
(`gh workflow run windows.yml --ref main`), which tests everything.

## IPC

- `contract/commands/<command>.json` holds the exact arguments `inventoryApi` sends and
  `contract/responses/` what Rust answers. `src/features/inventory/ipc-contract.test.ts` and
  `src-tauri/src/ipc_contract.rs` check each side against them. A new command or argument
  needs its fixture, its call in the TS test and its readers in `arguments()` in Rust.
- `contract/attention/` holds cases of «what needs my attention» (inventory, instant, expected
  ranking, coverage and status). `src/domain/attention-contract.test.ts` and
  `src-tauri/src/attention.rs` check the window's rules and their Rust copy (for the agent
  gate) against them: change a rule on both sides and in the cases together.
- `contract/pending-work/` does the same for pending work, what the readings of local copies and
  installations say is waiting: `src/features/inventory/pending-contract.test.ts` and
  `src-tauri/src/pending.rs` (for `attention` in the agent gate) rank its cases. It is not health
  and never touches `contract/attention/`.
- Tauri runs a synchronous command on the thread that received the request: the main thread,
  which has no Tokio runtime. A command that spawns a task or waits must be `async`.

## Agent gate

`devhq --mcp` (`src-tauri/src/gate.rs`) serves the inventory to an MCP client over stdio: no
window, no port, alive only while the client keeps stdin open. Every tool calls the same use cases
as the window's commands; never give it a shortcut around their validation. No tool that is not
`readOnlyHint` runs while the system keeps the file elsewhere (`storedAt`, `location_redirected`) or
while the previous location still holds the person's inventory (`move_pending`): `location`
decides it in `call`, before anything opens, whatever the access. Before a tool runs or
anything opens, its arguments are checked against the `inputSchema` it publishes and the rules its
description states (`gate/arguments.rs`): a schema keyword the check does not enforce fails a test,
so never publish a rule the gate does not apply, and state every rule between arguments in the
tool's description. It never creates an inventory that does not exist. It reads through
`inventory::examine` (`read_as_found`), which looks at the files before SQLite opens them, refuses one
that would need recovering (`needs_recovery`), serves only the current schema with its structure, and
opens only read-only connections SQLite cannot make write: `readonly_shm` for a log a connection
holds, `immutable` (checked again after the read) for a logged file without a log. One race remains:
if the last connection closes between the look and the opening, the `readonly_shm` connection creates
an empty `-wal`; so the README promises unchanged content, not untouched files. SQLite recovers,
folds, deletes and creates files on opening whatever `query_only` says, so never give a gate read
another kind of connection. Every write asks for the inventory and access through that read first,
then opens the file with `inventory::open_current` (configuration through `open_to_write`) and decides
in its transaction (`begin_write`): the schema and, for an agent, the access are checked again there,
under SQLite's write lock. An agent's write tries to empty the log once it commits (`Writing::commit`,
`empty_log`): a connection that closes while another reads cannot fold its log, and every gate refuses a
log nothing holds (`needs_recovery`), so no later write can fold it; only the window's start recovers it.
It retries for up to the busy timeout without waiting inside a try, so it never holds the write lock
while a reader finishes, and a fold that does not finish never turns the committed write into an error. On Unix `log_held` must never run while the process holds a connection to
the same database. While a window is open, its process notices what other connections
commit (`Monitor::notice_changes`, `PRAGMA data_version` every 2 s) and holds the lock that tells
`status` a window is open (`window_lock.rs`). The native walk drives the gate as a child process.
`--access <mode>` is a ceiling set where a client registers the gate: an agent may do the lower of it
and what Settings allows, never more. It travels with the agent (`Actor::Agent { ceiling }`), so
every check of an agent's access, the look before a write and the one inside its transaction, goes
through `access::effective` or `access::permitted_under`, never `access::mode` alone.
What remains to check or decide: `docs/puerta-para-agentes.md`. To try it
with a real client: `claude -p "..." --mcp-config <json> --strict-mcp-config --allowedTools
"mcp__devhq__*"` with the server's `--database` pointing at a test inventory.

## Opening the inventory

No opening changes the inventory's schema (`src-tauri/src/inventory.rs`, README «Arquitectura»).
`inventory::create` makes a new inventory only where no file is, built under its own name and linked
into place, never over an existing file, even an empty one, and never beside a `-journal`, `-wal` or
`-shm` left without its database (`leftover_files`): SQLite pairs those files with a database by name
alone and would apply them to the new one. `inventory::start` is the window's start
and the only caller of `create`, the only place that sets WAL and the only place that lets SQLite
recover a file, and only once a recovered copy in a temporary folder proved it current and, with the
connection that recovers it open and before its first read, the file, journal and log still hold the
bytes that were copied (`RecoveredFrom`); anything else is refused until DevHQ starts again or, for an
outdated inventory, until the person upgrades it (`InventoryPath::current`), and supervision does not
run.
`open_current` never creates, upgrades or converts. Every use case reads or writes inside `begin` or
`begin_write`, which classify the file inside the transaction; never start a transaction on an
inventory connection another way. The migrations build a new inventory and upgrade an older one only
in `src-tauri/src/upgrade.rs`, through the window's `upgrade_inventory` and the person's consent to
what `inspect_upgrade` showed (version and content fingerprint); never add another path that
upgrades, and never give the agent gate one. The upgrade looks with `examine` first (so on Unix it
and the inspection run only while the window holds no connection: the `Upgrades` mutex and the
`ready` check), opens `open_to_upgrade`, takes `BEGIN IMMEDIATE`, checks consent, structure
(`schema_differences`, stricter than `classify`), integrity and relationships inside it, backs up with
`VACUUM INTO` from a second read-only connection, verifies the copy against the inventory under the
lock, names it and its manifest (`backup::copy_verified`, `backup::write_new`), forces both names to
disk (`backup::names_on_disk`, which on Unix syncs the folders and on Windows has nothing to ask), and
only then migrates in the same transaction and checks every row (`check_upgraded`) before committing.
Any error the system reports in that publication stops the upgrade and keeps what was named; never
drop one. It never recovers an outdated inventory: one left needing recovery is refused with its files
left as they are, and no message may presume that a backup exists or holds what they hold, nor choose
one by its name (README «Un inventario antiguo que necesita recuperación»). In tests,
`inventory::upgrade` applies the migrations with no backup or check, to prove each of them on synthetic
files. `inventory::open` exists only in tests. A test inventory is made with `start`, as the window
makes it.

## Local copies

`src-tauri/src/local_copies.rs` reads a declared local copy with libgit2's read APIs, never with the
`git` program, which runs what a repository names. Each copy is read in `devhq --read-copy`, a process
that first forbids itself to start programs (a Job Object on Windows, seccomp on `execve` on Linux;
elsewhere it refuses to read) and that the caller kills at its deadline; its answer is checked as
untrusted. Before anything opens a declared path, `this_computer::refusal` decides from the path's
text, the drive's type and the links on the way whether it leads off this computer's disks (a share,
a network drive, a device): then the reading is `unreadable` and nothing is opened, because looking
at a share makes Windows connect to it and authenticate the person there. The copy reader also checks
`.git` (a link) and reads `.git` (`gitdir:`) and `commondir` as text before libgit2 opens the copy.
Never open a declared path,
or a path a copy names, without that check, and never add a reader that does. Not every libgit2 API
that sounds like reading only reads (the reflog API creates a missing
log): a new libgit2 call, a new reader or library, and every libgit2 upgrade must pass the hostile
battery (`local_copies/battery.rs`) on Linux and Windows. What is not guaranteed, and stays written in
the README: code that hostile data ran inside the reader has the person's rights, nothing of the
system stops the reader writing, and a path a repository names for libgit2 to open (`alternates`, an
`include.path`) is not checked. Which workstation this computer is lives in `this-workstation.json`
in the data folder, beside the inventory, keyed by a machine fingerprint, never in the inventory;
only the window chooses it. Tests build repositories with the installed `git`, without the machine's config, and
compile the battery's marker with the toolchain's `rustc`.

## Installations

`src-tauri/src/installations.rs` reads the installed executable's file that an `installation`
reference declares, as data: it never runs it (no `--version`), never loads it as an image and never
asks Windows' version APIs, which load it. Each file is read in `devhq --read-installation`, a
process that first forbids itself to start programs (`local_copies::confine`) and that the caller
kills at its deadline; its answer is checked as untrusted, against the same rules as the schema's
CHECKs. The version resource is read by `installations/pe.rs`, DevHQ's own reader, without `unsafe`
and without a crate: it trusts no number the file gives, walks the resource directory to a fixed
depth and bounds every loop. Any change to it, and any new structure it reads, must pass the hostile
battery (`installations/battery.rs`) on Linux and Windows, which also holds it to reading no more of
a file than its limits allow. The reader asks `this_computer::refusal` first, as the copy reader
does, then opens the declared file to read and nothing else, and where
the system really keeps it is asked with `inventory::stored_at`, never another way. To count the
processes running from the file it looks at the system's list of processes and keeps two numbers,
the second counting only the processes the system hid whose listed name is the file's: never a
process identifier, a name, a command line or another program's path. Tests use synthetic
images without code (`pe::tests::image`) and the test executable itself, never an installed program.

## Credentials

`src-tauri/src/credentials.rs` keeps DevHQ's only credential, the GitHub token
(`docs/leer-github.md`, specification «Token de GitHub»), in the system's store under
`<identifier>:github`: the Credential Manager on Windows, the kernel's session keyring on Linux, nothing
anywhere else. The window sends it once and only learns whether one is kept; never return it through
IPC, write it to the inventory, a backup, a log or an error, or let the agent gate reach it. Accepting
any other credential, or another kind of token, rewrites the product rule above first. Tests use the
in-memory store (`credentials::tests::in_memory`), never the system's, which may hold the person's
token; only `the_system_store_keeps_replaces_and_removes_a_secret` touches it, under a name of its
own that it removes when it ends.

## Version

DevHQ's version is one number in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`,
`src-tauri/Cargo.lock`, `src-tauri/tauri.conf.json` and `src/shared/version.ts`
(`src/shared/version.test.ts` holds them together). Raise it when an installer is built to be
installed, not with every pull request, and raise it in the last pull request of what that installer
gathers, never in a pull request of its own, which would pay a Windows run for one number. Install
when the person's use or another application (an agent client that needs a new tool or rule) needs
what was merged, not after every merge: several pull requests go in one installer. What tells one
build from another is the commit, which `src-tauri/build.rs` writes after the version in the Windows
version resource (`0.2.0+<12 digits>`). It reads the commit from `.git` as files
(`src-tauri/src/build_version.rs`, also where `.git` is a file: a linked worktree, a submodule) and
never runs `git` or any other program. It says `.dirty` only when the builder sets `DEVHQ_BUILD_DIRTY=1`, because it cannot know
without `git status`; and without a `.git` it understands, it adds no commit. Never make it guess.

## Schema

A schema change is a new migration (`src-tauri/migrations/00N_*.sql`, next `user_version`)
with an upgrade test that compares dumps row by row, like the existing ones. Never edit a
released migration. The explicit upgrade's `check_upgraded` must accept it: every row keeps its
rowid and values, a new column holds its declared default and a new table starts empty, unless the
migration says otherwise there (`MOVED`, `STARTED`) with a test of its own; and its upgrade test in
`upgrade.rs` covers the new version. Queries over automated history follow an index; a new one gets a
query plan test so its cost never grows with the history.

## Style

- One commit per coherent change. Messages in English: a short imperative title, then a
  body that explains why, wrapped at 72 columns, with the session trailers.
- Code comments in English, dense and explanatory, like the surrounding code; clear names.
- Specification in Spanish, written as prose that states behaviour precisely; `README.md` in
  Spanish too, short and for someone who has just found the repository.
