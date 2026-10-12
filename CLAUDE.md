# DevHQ

Native desktop app (Tauri 2): Rust + SQLite in `src-tauri/`, React + TypeScript + Vite +
Tailwind 4 in `src/`. The README (in Spanish) is the specification: read it before changing
behaviour, and update it in the same commit as the change. `AGENTS.md` sends Codex and other
agents here: keep every rule in this file and only pointers there.

## Product rules

- Every view answers «¿Qué necesita mi atención?». A feature that only saves typing a
  simple command does not belong here.
- Nothing empty, missing, unknown or never observed is presented as healthy or complete.
  When a list is cut (history window, paging), the UI says where and why.
- History is never deleted or rewritten: observations are immutable, subjects with history
  cannot be deleted, a schema change keeps every row.
- DevHQ observes; it does not operate systems, run in the background or hold credentials. It
  reads only what the inventory names, when asked, and never searches a disk or a network for things
  to add. Observing never gives the source a way to run code or change anything: a reader never runs
  a program the source names and never writes to it, and every declared path, and what it holds, is
  untrusted data.
- Provenance (`source`), observed and recorded times, and validity are always visible.
- `docs/vision.md` (in Spanish) holds the long-term direction and the questions to ask of any
  new feature or architectural change; read it before proposing either. It guides and
  authorizes nothing: the README stays the specification, and the rules above hold until a
  change that must break one rewrites that rule, here and in the README, in the same commit.

## Checks

Run all of them before every commit and look at each exit code. Never pipe a check through
`tail` or `head`: the pipeline's exit code is the last command's, which hides a failure.

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
`?scenario=empty` and `?scenario=unavailable` show an empty and an unreadable inventory.
To look at the UI: `npx vite --host 127.0.0.1 --port 1420`, then open
`http://127.0.0.1:1420/preview.html#/...` with the global Playwright (`npm root -g`).

## Native walk

`e2e/native/` drives the real app with a WebDriver client of its own over `fetch` (no
dependency): through tauri-driver and WebKitWebDriver on Linux; on Windows msedgedriver attaches
to the DevTools port that only the build from `e2e/native/build.ts` opens, because tauri-driver
cannot launch a Tauri app on WebView2 150 or later. It finds elements by XPath on visible text and labels, so renaming a
label, a button or a heading it uses means updating `main.ts`. It never touches a real
inventory: it drives only an executable the installer did not pack (development data, see
below); on Linux every XDG directory points into a temporary folder; on Windows it refuses
to run while `%APPDATA%` or `%LOCALAPPDATA%` already hold `com.devhq.desktop.dev`. It closes the window
through the system (X11 `WM_DELETE_WINDOW`, Win32 `WM_CLOSE`), never through WebDriver, whose
Close Window only destroys the WebView. Screenshots of each step land in `test-results/native/`.

## Development data

`src-tauri/src/identity.rs` names the data folders for the window and the gate alike: only the
executable the NSIS installer packed (Tauri's bundle marker) keeps `com.devhq.desktop`, the
person's inventory; `tauri dev`, debug builds, `target/release` and the tests keep
`com.devhq.desktop.dev`, and the window title says «development data». The marker says how the
file was built, not where it is installed, and it does not stop MSIX redirection or `--database`:
never point a development build at the person's inventory. Anything that resolves a data path
goes through `identity`.

## Local Windows sessions

The Claude and Codex desktop apps are packaged (MSIX), and Windows redirects AppData for every
process they start: `npm run tauri dev` and your own shell read and write the package's copy
(`%LOCALAPPDATA%\Packages\<package>\LocalCache\Roaming\com.devhq.desktop.dev`), merged with the real
`%APPDATA%`, while File Explorer and the installed DevHQ see only the real one. Settings shows
the path the file really has (`storedAt`). To look at or change the real `%APPDATA%`, run the
command outside the package, through WMI (`Invoke-CimMethod Win32_Process -MethodName Create`).

## Windows workflow

`.github/workflows/windows.yml` runs on demand and on pull requests, but its Windows jobs only run
when a push changed `src-tauri/`, `contract/`, `e2e/native/`, the workflow, `.github/windows/` or
`package-lock.json` since the newest commit of the branch on which every Windows job passed
(`.github/windows/needs-windows.sh`, which a Linux job runs first). A push that only touches `src/`
or documentation skips them. A run costs about 22 billable minutes with a warm cache and 48 cold:
group the changes it tests, push rarely, and do not push while a run whose result you need is in
progress, because a new push cancels it.

## IPC

- `contract/commands/<command>.json` holds the exact arguments `inventoryApi` sends and
  `contract/responses/` what Rust answers. `src/features/inventory/ipc-contract.test.ts` and
  `src-tauri/src/ipc_contract.rs` check each side against them. A new command or argument
  needs its fixture, its call in the TS test and its readers in `arguments()` in Rust.
- `contract/attention/` holds cases of «what needs my attention» (inventory, instant, expected
  ranking, coverage and status). `src/domain/attention-contract.test.ts` and
  `src-tauri/src/attention.rs` check the window's rules and their Rust copy (for the agent
  gate) against them: change a rule on both sides and in the cases together.
- Tauri runs a synchronous command on the thread that received the request: the main thread,
  which has no Tokio runtime. A command that spawns a task or waits must be `async`.

## Agent gate

`devhq --mcp` (`src-tauri/src/gate.rs`) serves the inventory to an MCP client over stdio: no
window, no port, alive only while the client keeps stdin open. Every tool calls the same use cases
as the window's commands; never give it a shortcut around their validation. Before a tool runs or
anything opens, its arguments are checked against the `inputSchema` it publishes and the rules its
description states (`gate/arguments.rs`): a schema keyword the check does not enforce fails a test,
so never publish a rule the gate does not apply, and state every rule between arguments in the
tool's description. It never creates an inventory that does not exist. It reads, and changes the
configuration, through `inventory::open_as_found`, which never creates, upgrades or converts the file
and serves only the current schema; never through `inventory::open`, which does all three. Records,
`run_check`, `backup` and `read_copies` still reach `inventory::open` through the window's use cases
after that check: closing that gap belongs to the explicit upgrade (`docs/puerta-para-agentes.md`). While a window is open, its process notices what other connections
commit (`Monitor::notice_changes`, `PRAGMA data_version` every 2 s) and holds the lock that tells
`status` a window is open (`window_lock.rs`). The native walk drives the gate as a child process.
What remains to check or decide: `docs/puerta-para-agentes.md`. To try it
with a real client: `claude -p "..." --mcp-config <json> --strict-mcp-config --allowedTools
"mcp__devhq__*"` with the server's `--database` pointing at a test inventory.

## Local copies

`src-tauri/src/local_copies.rs` reads a declared local copy with libgit2's read APIs, never with the
`git` program, which runs what a repository names. Each copy is read in `devhq --read-copy`, a process
that first forbids itself to start programs (a Job Object on Windows, seccomp on `execve` on Linux;
elsewhere it refuses to read) and that the caller kills at its deadline; its answer is checked as
untrusted. Not every libgit2 API that sounds like reading only reads (the reflog API creates a missing
log): a new libgit2 call, a new reader or library, and every libgit2 upgrade must pass the hostile
battery (`local_copies/battery.rs`) on Linux and Windows. What is not guaranteed, and stays written in
the README: code that hostile data ran inside the reader has the person's rights, and nothing of the
system stops the reader writing. Which workstation this computer is lives in `this-workstation.json`
in the local data folder, keyed by a machine fingerprint, never in the inventory; only the window
chooses it. Tests build repositories with the installed `git`, without the machine's config, and
compile the battery's marker with the toolchain's `rustc`.

## Schema

A schema change is a new migration (`src-tauri/migrations/00N_*.sql`, next `user_version`)
with an upgrade test that compares dumps row by row, like the existing ones. Never edit a
released migration. Queries over automated history follow an index; a new one gets a
query plan test so its cost never grows with the history.

## Style

- One commit per coherent change. Messages in English: a short imperative title, then a
  body that explains why, wrapped at 72 columns, with the session trailers.
- Code comments in English, dense and explanatory, like the surrounding code; clear names.
- README in Spanish, written as prose that states behaviour precisely.
