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
 * Where DevHQ keeps its data, isolated from any real inventory. Linux follows XDG, so the walk
 * points every XDG directory into a new temporary folder. Windows resolves its folders through
 * the shell's known-folder API, which no environment variable redirects: there the walk only
 * runs when neither DevHQ folder exists yet, and removes the ones it created. It never reads,
 * writes or removes a data directory it did not create.
 */
export async function isolate() {
  if (windows) {
    const { APPDATA, LOCALAPPDATA } = process.env
    if (!APPDATA || !LOCALAPPDATA) throw new Error('APPDATA and LOCALAPPDATA must be set.')
    const data = join(APPDATA, config.identifier)
    // WebView2 keeps its own profile here.
    const local = join(LOCALAPPDATA, config.identifier)
    for (const folder of [data, local])
      if (existsSync(folder))
        throw new Error(
          `${folder} already exists. The native walk would use it, so it refuses to run: it only ` +
            'runs where DevHQ has no data yet, such as a fresh CI runner.',
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
    dataDirectory: join(env.XDG_DATA_HOME, config.identifier),
    cleanup: async () => {
      if (!process.env.DEVHQ_E2E_KEEP_DATA)
        await rm(home, { recursive: true, force: true, maxRetries: 5 })
    },
  }
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

/**
 * The native WebDriver for this platform: WebKitWebDriver on Linux, found on the PATH by
 * tauri-driver, and on Windows the msedgedriver that DEVHQ_EDGEDRIVER names or, on GitHub's
 * runners, the one in EDGEWEBDRIVER, which must match the installed WebView2.
 */
function nativeDriver() {
  if (!windows) return []
  const explicit = process.env.DEVHQ_EDGEDRIVER
  const runner = process.env.EDGEWEBDRIVER && join(process.env.EDGEWEBDRIVER, 'msedgedriver.exe')
  const driver = explicit ?? (runner && existsSync(runner) ? runner : undefined)
  return driver ? ['--native-driver', driver] : []
}

/** Starts tauri-driver and, through it, DevHQ. */
export async function launch(binary: string, env: NodeJS.ProcessEnv, helpers: ChildProcess[]) {
  const [port, nativePort] = [await freePort(), await freePort()]
  const driver = track(
    spawn(
      process.env.DEVHQ_TAURI_DRIVER ?? 'tauri-driver',
      ['--port', String(port), '--native-port', String(nativePort), ...nativeDriver()],
      { env, stdio: ['ignore', 'inherit', 'inherit'] },
    ),
    helpers,
  )
  const exited = new Promise<never>((_, reject) => {
    driver.on('error', reject)
    driver.on('exit', (code) => reject(new Error(`tauri-driver exited with ${code}`)))
  })
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

/** The running process of this executable, if there is exactly one. */
function processOf(binary: string): number | undefined {
  if (windows) {
    const script = `(Get-CimInstance Win32_Process -Filter "Name='${binary.split('\\').pop()}'" | Where-Object ExecutablePath -eq '${binary.replaceAll("'", "''")}').ProcessId`
    const found = powershell(script).split(/\s+/).filter(Boolean)
    return found.length === 1 ? Number(found[0]) : undefined
  }
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
 */
export function closeWindow(pid: number, env: NodeJS.ProcessEnv) {
  if (windows) {
    powershell(`
      $ErrorActionPreference = 'Stop'
      Add-Type -Namespace DevHQ -Name User32 -MemberDefinition '[DllImport("user32.dll", SetLastError = true)] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr w, IntPtr l);'
      $window = (Get-Process -Id ${pid}).MainWindowHandle
      if ($window -eq [IntPtr]::Zero) { throw 'DevHQ has no main window' }
      if (-not [DevHQ.User32]::PostMessageW($window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) { throw 'PostMessage failed' }
    `)
    return
  }
  const result = spawnSync(
    'python3',
    [join(import.meta.dirname, 'close-x11-window.py'), String(pid), config.app.windows[0]!.title],
    { env, encoding: 'utf8' },
  )
  if (result.status !== 0)
    throw new Error(`Could not close the window: ${result.stderr || result.stdout}`)
}
