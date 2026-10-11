/**
 * Launches the real DevHQ (Rust, SQLite and the system WebView) under tauri-driver, against a
 * data directory the walk owns, and closes its window the way a person does. Linux drives
 * WebKitGTK through WebKitWebDriver; Windows drives WebView2 through msedgedriver.
 */
import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import { existsSync, readFileSync, readdirSync, readlinkSync, realpathSync } from 'node:fs'
import { mkdtemp, rm } from 'node:fs/promises'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { ready, Session } from './webdriver.ts'

const root = join(import.meta.dirname, '..', '..')
const config = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')) as {
  identifier: string
  app: { windows: { title: string }[] }
}
const windows = process.platform === 'win32'

/**
 * What every executable DevHQ's installer did not pack keeps its data under, and how its window
 * is titled (src-tauri/src/identity.rs): the walk drives only such executables.
 */
const identifier = `${config.identifier}.dev`
const title = `${config.app.windows[0]!.title} (development data)`

/**
 * The marker Tauri's bundler patches in the executable it packs into the installer: present as is,
 * the executable was never packed and keeps development data. An unpacked executable holds it once;
 * the packed one, none (it reads `..._NSS` there). The other `__TAURI_BUNDLE_TYPE_VAR_` strings are
 * the values `bundle_type` compares against, in every executable (the unpacked release one holds
 * `..._NSS` too), so only this one tells them apart.
 */
const UNPACKED = Buffer.from('__TAURI_BUNDLE_TYPE_VAR_UNK')

/**
 * Refuses, from the file alone and before anything runs or any folder is looked at, an
 * executable that does not plainly say the installer never packed it: exactly one unpatched
 * marker. None means the installer packed it, or that it carries no marker this walk knows;
 * several leave unclear which one Tauri reads. Either way it could keep the installed DevHQ's data.
 */
export function refusePacked(binary: string) {
  const file = readFileSync(binary)
  let found = 0
  for (let at = file.indexOf(UNPACKED); at >= 0; at = file.indexOf(UNPACKED, at + 1)) found++
  if (found === 1) return
  throw new Error(
    found === 0
      ? `${binary} has no unpatched bundle marker: DevHQ's installer packed it, so it keeps the ` +
          "installed DevHQ's data, or it is not a DevHQ executable this walk knows."
      : `${binary} has ${found} unpatched bundle markers, so which one Tauri reads is unclear.` +
          ' The native walk drives only executables that plainly were never packed.',
  )
}

/** The executable under test: DEVHQ_APP, or the debug build `npm run e2e:native` makes. */
export function appBinary() {
  const binary =
    process.env.DEVHQ_APP ??
    join(root, 'src-tauri', 'target', 'debug', windows ? 'devhq.exe' : 'devhq')
  if (!existsSync(binary))
    throw new Error(`${binary} does not exist. Build it with npm run e2e:native, or set DEVHQ_APP.`)
  return realpathSync(binary)
}

/**
 * Where DevHQ keeps its data, isolated from any real inventory. The walk drives only an
 * executable DevHQ's installer did not pack, whose data is development data, apart from the
 * installed DevHQ's. Linux follows XDG, so the walk points every XDG directory into a new
 * temporary folder. Windows resolves its folders through the shell's known-folder API, which no
 * environment variable redirects: there the walk only runs when neither development folder
 * exists yet, and removes the ones it created. It never reads, writes or removes a data directory
 * it did not create. Then it asks the executable's agent gate where the window will keep its
 * inventory, which must be that folder.
 */
export async function isolate(binary: string) {
  refusePacked(binary)
  const isolated = windows ? await isolateWindows() : await isolateLinux()
  const asked = await databaseOf(binary, isolated.env)
  const expected = join(isolated.dataDirectory, 'inventory.sqlite3')
  // Windows compares names regardless of case.
  if (windows ? asked.toLowerCase() !== expected.toLowerCase() : asked !== expected)
    throw new Error(`${binary} would keep its inventory at ${asked}, not at ${expected}.`)
  return isolated
}

async function isolateWindows() {
  const { APPDATA, LOCALAPPDATA } = process.env
  if (!APPDATA || !LOCALAPPDATA) throw new Error('APPDATA and LOCALAPPDATA must be set.')
  const data = join(APPDATA, identifier)
  // WebView2 keeps its own profile here.
  const local = join(LOCALAPPDATA, identifier)
  for (const folder of [data, local])
    if (existsSync(folder))
      throw new Error(
        `${folder} already exists. The native walk would use it, so it refuses to run: it only ` +
          'runs where DevHQ has no development data yet, such as a fresh CI runner.',
      )
  return {
    env: { ...process.env },
    dataDirectory: data,
    cleanup: async () => {
      if (process.env.DEVHQ_E2E_KEEP_DATA) return
      for (const folder of [data, local])
        await rm(folder, { recursive: true, force: true, maxRetries: 5 })
    },
  }
}

async function isolateLinux() {
  if (process.platform !== 'linux') throw new Error('tauri-driver supports Linux and Windows only.')
  const home = await mkdtemp(join(tmpdir(), 'devhq-native-'))
  const env = {
    ...process.env,
    XDG_DATA_HOME: join(home, 'data'),
    XDG_CONFIG_HOME: join(home, 'config'),
    XDG_CACHE_HOME: join(home, 'cache'),
    XDG_STATE_HOME: join(home, 'state'),
  }
  return {
    env,
    dataDirectory: join(env.XDG_DATA_HOME, identifier),
    cleanup: async () => {
      if (!process.env.DEVHQ_E2E_KEEP_DATA)
        await rm(home, { recursive: true, force: true, maxRetries: 5 })
    },
  }
}

/**
 * The inventory the executable's window opens, as its agent gate's `status` names it when no
 * `--database` is given. The folder does not exist yet, so the gate opens nothing and creates
 * nothing: it only says where.
 */
async function databaseOf(binary: string, env: NodeJS.ProcessEnv) {
  const messages = [
    { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18' } },
    { jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'status' } },
  ]
  const result = spawnSync(binary, ['--mcp'], {
    env,
    input: messages.map((message) => `${JSON.stringify(message)}\n`).join(''),
    encoding: 'utf8',
    timeout: 30_000,
  })
  if (result.status !== 0)
    throw new Error(`${binary} --mcp failed (${result.status}): ${result.stderr}`)
  const replies = result.stdout
    .split('\n')
    .filter((line) => line.trim())
    .map((line) => JSON.parse(line) as { id: number; result?: { structuredContent?: Status } })
  const status = replies.find((reply) => reply.id === 2)?.result?.structuredContent
  if (!status) throw new Error(`The agent gate gave no status: ${result.stdout}`)
  if (status.exists) throw new Error(`${status.database.database} already holds an inventory.`)
  return status.database.database
}

interface Status {
  database: { database: string }
  exists: boolean
}

/** Remembers a helper process, so that the walk stops it however it ends. */
function track(child: ChildProcess, helpers: ChildProcess[]) {
  helpers.push(child)
  return child
}

export async function stopAll(helpers: ChildProcess[]) {
  for (const child of helpers.reverse()) if (child.exitCode === null) child.kill()
}

/** On Linux without a display, a private Xvfb server; it picks a free display number itself. */
export async function display(env: NodeJS.ProcessEnv, helpers: ChildProcess[]) {
  if (windows || env.DISPLAY) return
  const xvfb = track(
    spawn('Xvfb', ['-displayfd', '3', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], {
      stdio: ['ignore', 'ignore', 'pipe', 'pipe'],
    }),
    helpers,
  )
  // Xvfb warns about every keysym its keymap lacks; that only matters if it fails to start.
  let log = ''
  xvfb.stderr!.on('data', (chunk: Buffer) => (log = (log + chunk.toString()).slice(-4000)))
  const number = await new Promise<string>((resolve, reject) => {
    let out = ''
    xvfb.stdio[3]!.on('data', (chunk: Buffer) => {
      out += chunk.toString()
      if (out.includes('\n')) resolve(out.trim())
    })
    xvfb.on('error', reject)
    xvfb.on('exit', (code) => reject(new Error(`Xvfb exited with ${code}:\n${log}`)))
  })
  env.DISPLAY = `:${number}`
}

async function freePort() {
  return new Promise<number>((resolve, reject) => {
    const server = createServer()
    server.on('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address() as { port: number }
      server.close(() => resolve(port))
    })
  })
}

/** Polls until `check` returns something truthy, or fails with what it was waiting for. */
export async function until<T>(what: string, check: () => Promise<T> | T, timeout = 15_000) {
  const deadline = Date.now() + timeout
  let last: unknown
  for (;;) {
    try {
      const value = await check()
      if (value) return value
    } catch (error) {
      last = error
    }
    if (Date.now() > deadline)
      throw new Error(`Timed out after ${timeout} ms waiting for ${what}`, { cause: last })
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
}

/** The DevTools port the Windows test build opens in its WebView2 (see build.ts). */
export const WEBVIEW2_DEBUG_PORT = 9222

/** A process that stops the walk if it ends before the walk is done with it. */
function exitOf(child: ChildProcess, name: string) {
  return new Promise<never>((_, reject) => {
    child.on('error', reject)
    child.on('exit', (code) => reject(new Error(`${name} exited with ${code}`)))
  })
}

/**
 * Starts DevHQ and a WebDriver session on its window. Linux: tauri-driver starts
 * WebKitWebDriver, which launches DevHQ. Windows: tauri-driver cannot have msedgedriver launch
 * a Tauri app on WebView2 150 or later (see build.ts), so the walk starts DevHQ itself and
 * msedgedriver attaches to the DevTools port of the test build.
 */
export async function launch(binary: string, env: NodeJS.ProcessEnv, helpers: ChildProcess[]) {
  return windows ? attach(binary, env, helpers) : viaTauriDriver(binary, env, helpers)
}

async function viaTauriDriver(binary: string, env: NodeJS.ProcessEnv, helpers: ChildProcess[]) {
  const [port, nativePort] = [await freePort(), await freePort()]
  const driver = track(
    spawn(
      process.env.DEVHQ_TAURI_DRIVER ?? 'tauri-driver',
      ['--port', String(port), '--native-port', String(nativePort)],
      { env, stdio: ['ignore', 'inherit', 'inherit'] },
    ),
    helpers,
  )
  const exited = exitOf(driver, 'tauri-driver')
  // The native driver first: tauri-driver logs an error for every request it cannot forward.
  await Promise.race([
    until('the native WebDriver to answer', () => ready(`http://127.0.0.1:${nativePort}`), 30_000),
    exited,
  ])
  const base = `http://127.0.0.1:${port}`
  await Promise.race([until('tauri-driver to answer', () => ready(base), 30_000), exited])
  const session = await Promise.race([
    Session.create(base, { 'tauri:options': { application: binary } }),
    exited,
  ])
  const pid = await until('the DevHQ process', () => processOf(binary), 30_000)
  return { session, pid }
}

/**
 * msedgedriver must be the same version as WebView2: DEVHQ_EDGEDRIVER names it, or else the one
 * GitHub's runners keep in EDGEWEBDRIVER, or else the one on the PATH.
 */
function edgeDriver() {
  if (process.env.DEVHQ_EDGEDRIVER) return process.env.DEVHQ_EDGEDRIVER
  const runner = process.env.EDGEWEBDRIVER && join(process.env.EDGEWEBDRIVER, 'msedgedriver.exe')
  return runner && existsSync(runner) ? runner : 'msedgedriver'
}

async function attach(binary: string, env: NodeJS.ProcessEnv, helpers: ChildProcess[]) {
  const app = track(spawn(binary, [], { env, stdio: 'inherit' }), helpers)
  const appExited = exitOf(app, 'DevHQ')
  const debuggerAddress = `127.0.0.1:${WEBVIEW2_DEBUG_PORT}`
  await Promise.race([
    until(
      'the WebView2 DevTools port (is this the build from e2e/native/build.ts?)',
      async () => (await fetch(`http://${debuggerAddress}/json/version`)).ok,
      60_000,
    ),
    appExited,
  ])
  const port = await freePort()
  const driver = track(
    spawn(edgeDriver(), [`--port=${port}`], { stdio: ['ignore', 'inherit', 'inherit'] }),
    helpers,
  )
  const stopped = Promise.race([appExited, exitOf(driver, 'msedgedriver')])
  const base = `http://127.0.0.1:${port}`
  await Promise.race([until('msedgedriver to answer', () => ready(base), 30_000), stopped])
  const session = await Promise.race([
    Session.create(base, { browserName: 'webview2', 'ms:edgeOptions': { debuggerAddress } }),
    stopped,
  ])
  // From here on the walk watches DevHQ's exit itself; the cleanup stops msedgedriver.
  stopped.catch(() => {})
  return { session, pid: app.pid! }
}

/** The running process of this executable on Linux, if there is exactly one. */
function processOf(binary: string): number | undefined {
  const found = readdirSync('/proc')
    .filter((entry) => /^\d+$/.test(entry))
    .filter((pid) => {
      try {
        return readlinkSync(`/proc/${pid}/exe`) === binary
      } catch {
        return false
      }
    })
  return found.length === 1 ? Number(found[0]) : undefined
}

/** Whether the process still runs. An exited child its parent has not reaped yet has ended. */
export function alive(pid: number) {
  if (!windows) {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, 'utf8')
      // The state follows the parenthesized command name: Z is a zombie, X is dead.
      return !/^\S+ \(.*\) [ZX] /.test(stat)
    } catch {
      return false
    }
  }
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

/**
 * What the system clipboard holds, where the walk can read it: on Windows through PowerShell.
 * Xvfb has no clipboard reader installed, so on Linux the window's own report has to do.
 */
export function clipboard(): string | undefined {
  if (!windows) return undefined
  return powershell('Get-Clipboard -Raw')
}

/** Ends the process if it still runs, and waits until it has. */
export async function kill(pid: number) {
  if (!alive(pid)) return
  try {
    process.kill(pid, 'SIGKILL')
  } catch {
    // It ended in between.
  }
  await until(`process ${pid} to end`, () => !alive(pid), 10_000)
}

function powershell(script: string) {
  const result = spawnSync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-Command', script],
    { encoding: 'utf8' },
  )
  if (result.status !== 0) throw new Error(`PowerShell failed: ${result.stderr || result.stdout}`)
  return result.stdout.trim()
}

/**
 * Asks the window to close as its close button does, never through WebDriver: on Windows a
 * WM_CLOSE to its main window, on X11 the WM_DELETE_WINDOW message a window manager sends.
 * Under Xvfb there is no window manager, so a short Python script sends it through libX11.
 * Either way the window must carry the title that says it keeps development data.
 */
export function closeWindow(pid: number, env: NodeJS.ProcessEnv) {
  if (windows) {
    powershell(`
      $ErrorActionPreference = 'Stop'
      Add-Type -Namespace DevHQ -Name User32 -MemberDefinition '[DllImport("user32.dll", SetLastError = true)] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr w, IntPtr l);'
      $process = Get-Process -Id ${pid}
      $window = $process.MainWindowHandle
      if ($window -eq [IntPtr]::Zero) { throw 'DevHQ has no main window' }
      if ($process.MainWindowTitle -cne '${title}') { throw "The window is titled '$($process.MainWindowTitle)', not '${title}'" }
      if (-not [DevHQ.User32]::PostMessageW($window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) { throw 'PostMessage failed' }
    `)
    return
  }
  const result = spawnSync(
    'python3',
    [join(import.meta.dirname, 'close-x11-window.py'), String(pid), title],
    { env, encoding: 'utf8' },
  )
  if (result.status !== 0)
    throw new Error(`Could not close the window: ${result.stderr || result.stdout}`)
}
