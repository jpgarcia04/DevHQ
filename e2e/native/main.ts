/**
 * The native end-to-end walk: the real DevHQ, with its Rust process, its SQLite file and the
 * system WebView, driven through tauri-driver like a person would use it. Everything else
 * (Vitest, the Playwright preview, the Rust tests on Tauri's mock runtime) stops short of the
 * real window; this walk is the one test that crosses it.
 *
 * It starts from a schema v6 inventory in rollback-journal mode, as DevHQ files were before
 * write-ahead logging, in a data directory of its own (see `isolate`). It drives only an
 * executable DevHQ's installer did not pack, which keeps development data, never touches a real
 * inventory and talks to no server but its own, on 127.0.0.1.
 *
 * Run it with `npm run e2e:native`, which builds the debug app first. DEVHQ_APP points it at
 * another build. Screenshots of every step land in test-results/native/.
 *
 * It also starts the same executable as the agent gate (`--mcp --database <its inventory>`) and
 * speaks MCP with it as Claude Code does, to check that the open window shows by itself what an
 * agent changes.
 */
import assert from 'node:assert/strict'
import { spawn, type ChildProcess } from 'node:child_process'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { copyFile, mkdir, mkdtemp, readdir, rm, writeFile } from 'node:fs/promises'
import { createServer, type RequestListener } from 'node:http'
import { createServer as createHttpsServer } from 'node:https'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import {
  alive,
  appBinary,
  clipboard,
  closeWindow,
  display,
  isolate,
  kill,
  launch,
  stopAll,
  until,
  WEBVIEW2_DEBUG_PORT,
} from './app.ts'
import { hostileCopy } from './hostile-copy.ts'
import { isTransient, reference, type Element, type Session } from './webdriver.ts'

const root = join(import.meta.dirname, '..', '..')
const shots = join(root, 'test-results', 'native')

// A string as an XPath literal, whatever quotes it holds.
const literal = (text: string) =>
  !text.includes("'")
    ? `'${text}'`
    : !text.includes('"')
      ? `"${text}"`
      : `concat('${text.replaceAll("'", `', "'", '`)}')`

const button = (name: string, scope = '') => `${scope}//button[normalize-space()=${literal(name)}]`
/** The input or select a label wraps, by the label's own text. */
const field = (label: string, scope = '') =>
  `${scope}//label[normalize-space(text()[1])=${literal(label)}]//*[self::input or self::select]`
const region = (name: string) => `//section[@aria-label=${literal(name)}]`
const form = (heading: string) => `//form[.//h2[normalize-space()=${literal(heading)}]]`

/** The window, as the steps use it: find, click, type and read like a person. */
function window(session: Session) {
  const find = (xpath: string, what = xpath) =>
    until(what, async () => (await session.findAll(xpath))[0])
  // The element can be re-rendered between finding and acting on it; then find it again.
  const act = (what: string, action: () => Promise<unknown>) =>
    until(what, async () => {
      try {
        await action()
        return true
      } catch (error) {
        if (isTransient(error)) return false
        throw error
      }
    })
  // Content that loads late pushes elements down. WebDriver aims a click at where the element
  // is when it computes the click, so an element still moving can be missed; wait until its box
  // stays put for a frame, as a person sees it settle before clicking.
  const settled = (element: Element) =>
    until('the element to stop moving', () =>
      session.execute<boolean>(
        `const box = () => JSON.stringify(arguments[0].getBoundingClientRect())
        const before = box()
        return new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done(box() === before))))`,
        [reference(element)],
      ),
    )
  const press = async (xpath: string) => {
    const element = await find(xpath)
    await settled(element)
    await session.click(element)
  }
  return {
    find,
    click: (xpath: string) => act(`to click ${xpath}`, () => press(xpath)),
    fill: (xpath: string, text: string) =>
      act(`to fill ${xpath}`, async () => {
        const element = await find(xpath)
        await session.clear(element)
        await session.type(element, text)
      }),
    /** Picks an option of a select by its text, which fires the select's change event. */
    choose: (select: string, option: string) =>
      act(`to choose ${option}`, () =>
        press(`${select}/option[normalize-space()=${literal(option)}]`),
      ),
    /** Waits until the text shows inside the scope, the page's main content by default. */
    see: (text: string, scope = '//main', timeout?: number) =>
      until(
        `"${text}" in ${scope}`,
        async () =>
          (await session.findAll(`${scope}[contains(normalize-space(.), ${literal(text)})]`))
            .length > 0,
        timeout,
      ),
    text: async (xpath: string) => session.text(await find(xpath)),
    /** Follows a link of the sidebar. */
    go: (label: string) =>
      act(`to open ${label}`, () =>
        press(`//aside//a[@aria-label=${literal(label)} or normalize-space()=${literal(label)}]`),
      ),
  }
}

const migrations = join(root, 'src-tauri', 'migrations')
/** Migration files by the schema version each one brings, oldest first. */
const migrationFiles = readdirSync(migrations)
  .filter((file) => /^\d{3}_.*\.sql$/.test(file))
  .sort()
const schemaVersion = migrationFiles.length

/**
 * A schema v6 inventory in rollback-journal mode, as every DevHQ file was before it used
 * write-ahead logging: the walk checks that the real app refuses it and changes none of its bytes,
 * since no DevHQ upgrades an inventory when it opens it.
 */
function seedVersion6(database: string) {
  const db = new DatabaseSync(database)
  for (const file of migrationFiles.slice(0, 6))
    db.exec(readFileSync(join(migrations, file), 'utf8'))
  db.exec('PRAGMA user_version = 6')
  db.close()
}

/** Bytes 18 and 19 of an SQLite file say how it journals: 1 rollback, 2 write-ahead log. */
const journalOf = (database: string) => {
  const header = readFileSync(database).subarray(18, 20)
  return header[0] === 2 && header[1] === 2 ? 'wal' : header[0] === 1 ? 'rollback' : 'unknown'
}

/**
 * The one server the HTTP check talks to. It counts what DevHQ asked of it. With
 * DEVHQ_E2E_TLS_PFX (a certificate for 127.0.0.1 that the system trusts, as the Windows
 * workflow sets up) it speaks HTTPS, so the check must complete a handshake with the system's
 * own TLS stack and trust store.
 */
async function healthServer() {
  const requests: { method?: string; url?: string; headers: Record<string, unknown> }[] = []
  const answer: RequestListener = (request, response) => {
    requests.push({ method: request.method, url: request.url, headers: request.headers })
    response.writeHead(200, { 'content-type': 'text/plain' }).end('ok')
  }
  const pfx = process.env.DEVHQ_E2E_TLS_PFX
  const server = pfx
    ? createHttpsServer(
        { pfx: readFileSync(pfx), passphrase: process.env.DEVHQ_E2E_TLS_PASSPHRASE },
        answer,
      )
    : createServer(answer)
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as { port: number }
  const scheme = pfx ? 'https' : 'http'
  return { url: `${scheme}://127.0.0.1:${port}/health`, requests, close: () => server.close() }
}

/**
 * The agent gate of the executable under test, driven as an MCP client drives it: one JSON-RPC
 * message per line on its stdin and stdout. It names itself `native-walk`, so what it writes is
 * tagged «Agent · native-walk».
 */
function agentGate(binary: string, database: string, env: NodeJS.ProcessEnv) {
  const child = spawn(binary, ['--mcp', '--database', database], {
    env,
    stdio: ['pipe', 'pipe', 'inherit'],
  })
  helpers.push(child)
  const replies: { id: number; result?: Record<string, unknown>; error?: unknown }[] = []
  let pending = ''
  child.stdout!.on('data', (chunk: Buffer) => {
    pending += chunk.toString()
    for (let end = pending.indexOf('\n'); end >= 0; end = pending.indexOf('\n')) {
      replies.push(JSON.parse(pending.slice(0, end)))
      pending = pending.slice(end + 1)
    }
  })
  const exited = new Promise<number | null>((resolve) => child.on('exit', resolve))
  let last = 0
  const request = async (method: string, params: Record<string, unknown> = {}) => {
    const id = ++last
    child.stdin!.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`)
    const reply = await until(`the gate to answer ${method}`, () =>
      replies.find((item) => item.id === id),
    )
    assert.equal(reply.error, undefined, `${method}: ${JSON.stringify(reply.error)}`)
    return reply.result!
  }
  /** A tool's answer, which must not be an error. */
  const tool = async (name: string, args: Record<string, unknown> = {}) => {
    const result = await request('tools/call', { name, arguments: args })
    assert.equal(result.isError, false, `${name}: ${JSON.stringify(result.content)}`)
    return result.structuredContent as Record<string, unknown>
  }
  return {
    request,
    tool,
    /** Closes its input, as a client does when its session ends: the gate must end. */
    close: async () => {
      child.stdin!.end()
      return exited
    },
  }
}

const helpers: ChildProcess[] = []
const isolated = await isolate(appBinary())
// The hostile copy lives outside DevHQ's data, as a person's working copies do.
const copies = await mkdtemp(join(tmpdir(), 'devhq-copies-'))
const server = await healthServer()
let session: Session | undefined
let pid: number | undefined
let failed = false
let count = 0

/**
 * Tells a slow native process from a slow page or driver: whether the page is visible, how long
 * it has been loaded, and when each call into DevHQ was sent and answered, in ms since the page
 * loaded. A call gets its entry only once answered, so one still waiting is missing.
 *
 * The page times only HTTP(S) requests (Resource Timing). Tauri builds the URL of each call with
 * `convertFileSrc(command, 'ipc')`: `http://ipc.localhost/<command>` on WebView2, which is timed,
 * and `ipc://localhost/<command>` on WebKitGTK, which is not. There the state says so, instead of
 * reporting that no call was answered.
 */
const windowState = (session: Session) =>
  session
    .execute<string>(
      `const ipc = window.__TAURI_INTERNALS__.convertFileSrc('', 'ipc')
      const calls = performance.getEntriesByType('resource')
        .filter((entry) => entry.name.startsWith(ipc))
        .slice(-20)
        .map((entry) => new URL(entry.name).pathname.slice(1) + ' ' + Math.round(entry.startTime) + '→' + Math.round(entry.responseEnd))
      return [
        'visibility ' + document.visibilityState + ', focus ' + document.hasFocus() + ', loaded ' + Math.round(performance.now()) + ' ms ago',
        /^https?:/.test(ipc)
          ? 'answered calls into DevHQ: ' + (calls.join(', ') || 'none recorded')
          : 'calls into DevHQ: not timed here, since Tauri sends them to ' + ipc + ', which is not HTTP(S), and the page times only HTTP(S) requests',
      ].join('\\n')`,
    )
    .catch(() => '(the window did not answer)')

async function step(name: string, body: () => Promise<void>) {
  const started = Date.now()
  count += 1
  try {
    await body()
  } catch (error) {
    failed = true
    console.error(`✗ ${name}`)
    // The window may be gone, which is often why the step failed: keep the step's own error.
    if (session) {
      await session
        .screenshot()
        .then((png) => writeFile(join(shots, 'failure.png'), png))
        .catch(() => {})
      const text = await session
        .execute<string>('return document.body.innerText')
        .catch(() => '(the window did not answer)')
      console.error(`--- window text ---\n${text}\n---`)
      console.error(`--- window state ---\n${await windowState(session)}\n---`)
    }
    throw error
  }
  if (session) {
    // A screenshot taken mid-transition shows the previous page's highlight.
    await until(
      'animations to settle',
      () =>
        session!.execute<boolean>(
          "return document.getAnimations().every((item) => item.playState !== 'running')",
        ),
      3_000,
    ).catch(() => {})
    await writeFile(
      join(
        shots,
        `${String(count).padStart(2, '0')}-${name.replaceAll(/\W+/g, '-').toLowerCase()}.png`,
      ),
      await session.screenshot(),
    )
  }
  console.log(`✓ ${name} (${Date.now() - started} ms)`)
}

try {
  await rm(shots, { recursive: true, force: true })
  await mkdir(shots, { recursive: true })
  const binary = appBinary()
  const { env, dataDirectory } = isolated
  const database = join(dataDirectory, 'inventory.sqlite3')
  // Where the v6 inventory goes once DevHQ has refused it: outside the data folder, in the walk's
  // own temporary folder, which it removes at the end.
  const aside = join(copies, 'inventory-v6.sqlite3')
  await mkdir(dataDirectory, { recursive: true })
  seedVersion6(database)
  assert.equal(journalOf(database), 'rollback')
  const older = readFileSync(database)
  await display(env, helpers)
  console.log(`DevHQ: ${binary}\nData: ${dataDirectory}`)
  let launched = Date.now()
  ;({ session, pid } = await launch(binary, env, helpers))
  console.log(`The window could be driven ${Date.now() - launched} ms after DevHQ started`)
  let ui = window(session)
  const resource = region('Build server')
  /**
   * The inventory's own files and its backups folder: everything beside the inventory that DevHQ
   * writes for it. The window's lock is left out, since the window holds it while it lives, and so
   * is the rest of the folder, which on Linux is also the local data folder where WebKitGTK keeps
   * its profile.
   */
  const dataFiles = async () =>
    (await readdir(dataDirectory))
      .filter((name) => name === 'backups' || name.startsWith('inventory.sqlite3'))
      .filter((name) => !name.endsWith('.window-lock'))
      .sort()

  await step('An older inventory is refused and left exactly as it was', async () => {
    await ui.see('Inventory needs a schema upgrade')
    await ui.see('declares schema version 6')
    await ui.find("//button[@aria-pressed='true' and normalize-space()='My inventory']")
    assert.ok(alive(pid!), 'DevHQ is running')
    // Back up now never upgrades an inventory to copy it, and leaves no folder behind.
    await ui.go('Settings')
    await ui.see(database, "//section[@aria-labelledby='database-file-heading']")
    await ui.click(button('Back up now'))
    await ui.see('No backup was made.')
    await ui.see('declares schema version 6')
    assert.deepEqual(await dataFiles(), ['inventory.sqlite3'], 'no file was added')
    assert.ok(readFileSync(database).equals(older), 'the v6 file changed')
  })

  await step('Closed and started again where no inventory is, it creates a new one', async () => {
    closeWindow(pid!, env)
    await until('DevHQ to exit', () => !alive(pid!), 20_000)
    await session!.end()
    session = undefined
    assert.ok(readFileSync(database).equals(older), 'the v6 file changed when DevHQ closed')
    // Set aside, so that its bytes can be checked at the end; nothing reads it again.
    await copyFile(database, aside)
    await rm(database)
    // On Windows the next DevHQ opens the same DevTools port: wait until the WebView2 of the one
    // that closed has let it go, so that the walk never attaches to a browser on its way out.
    if (process.platform === 'win32')
      await until(
        'the closed WebView2 to let go of its DevTools port',
        async () =>
          !(await fetch(`http://127.0.0.1:${WEBVIEW2_DEBUG_PORT}/json/version`).then(
            () => true,
            () => false,
          )),
        30_000,
      )
    launched = Date.now()
    ;({ session, pid } = await launch(binary, env, helpers))
    console.log(`The window could be driven ${Date.now() - launched} ms after DevHQ started`)
    ui = window(session)
    await ui.see('Nothing is registered yet.')
    await ui.find("//button[@aria-pressed='true' and normalize-space()='My inventory']")
    assert.equal(journalOf(database), 'wal', 'the new inventory writes ahead')
    // How long the first load took, creation included: the baseline for a slow one.
    console.log((await windowState(session!)).replaceAll(/^/gm, '  '))
  })

  await step('Settings shows the database path and copies it', async () => {
    await ui.go('Settings')
    const location = "//section[@aria-labelledby='database-file-heading']"
    await ui.see(database, location)
    await ui.click(button('Copy path', location))
    await ui.see('Path copied.', location)
    const copied = clipboard()
    if (copied !== undefined) assert.equal(copied, database)
  })

  await step('Create a resource', async () => {
    await ui.go('Infrastructure')
    await ui.click(button('New resource', '//main//header'))
    const resourceForm = form('New resource')
    await ui.fill(field('Name', resourceForm), 'Build server')
    await ui.choose(field('Kind', resourceForm), 'Server')
    await ui.fill(field('Description', resourceForm), 'Runs the nightly builds')
    await ui.fill(field('Operating system', resourceForm), 'Ubuntu 24.04')
    await ui.click(button('Save resource', resourceForm))
    await ui.see('Runs the nightly builds · Ubuntu 24.04', resource)
  })

  await step('Create an application on that resource', async () => {
    await ui.go('Applications')
    await ui.click(button('New application', '//main//header'))
    const applicationForm = form('New application')
    await ui.fill(field('Name', applicationForm), 'Status page')
    await ui.fill(field('Version', applicationForm), '1.4.0')
    await ui.click(button('Add component', applicationForm))
    await ui.fill(field('Component name', applicationForm), 'web')
    await ui.choose(field('Resource', applicationForm), 'Build server')
    // Where it is: a new reference starts as a repository, which is a URL on no machine.
    await ui.click(button('Add reference', applicationForm))
    await ui.fill(field('URL or path', applicationForm), 'https://git.example/status-page')
    await ui.click(button('Save application', applicationForm))
    await ui.see('1 component · 1 linked resource', "//main//a[@aria-label='Open Status page']")
  })

  await step('Record an observation', async () => {
    await ui.go('Infrastructure')
    await ui.click(button('Record observation', `${resource}//*[@role='group']`))
    const observation = `${resource}//form[@aria-label='Record observation']`
    await ui.choose(field('Result', observation), 'Healthy')
    await ui.fill(field('What did you observe?', observation), 'Disk at 40%, builds green')
    await ui.click(button('Save observation', observation))
    await ui.see('Disk at 40%, builds green', resource)
    await ui.see('Manual entry', resource)
  })

  await step('Supervise an HTTP check of a server on 127.0.0.1', async () => {
    await ui.click(button('Configure HTTP check', resource))
    const check = `${resource}//form[@aria-label='Configure HTTP check']`
    await ui.fill(field('URL', check), server.url)
    await ui.choose(field('Supervise while DevHQ is open', check), 'Every 5 min')
    await ui.click(button('Save HTTP check', check))
    // A supervised check that never ran is due at once. The run and the reload of its evidence
    // reach the window only through the status channel that watch_http_monitor opened.
    await until('DevHQ to request the health URL', () => server.requests.length >= 1)
    await ui.see('Automated · HTTP check', resource)
    await ui.see('HTTP 200 as expected', resource)
    await ui.see('Next run in', resource)
    const request = server.requests[0]!
    assert.equal(request.method, 'GET')
    assert.equal(request.url, '/health')
    assert.equal(request.headers.cookie, undefined)
  })

  await step('Run the check on demand', async () => {
    await ui.click(button('Run check', resource))
    await ui.see('Recorded as Healthy', resource)
    assert.equal(server.requests.length, 2)
  })

  await step('The Supervision panel shows the status DevHQ pushed', async () => {
    await ui.go('Overview')
    const panel = "//section[@aria-labelledby='supervision-heading']"
    await ui.see('1 of 2 systems are supervised', panel)
    await ui.see('Next run in', panel)
    await ui.see('Not supervised (1): Status page', panel)
    assert.doesNotMatch(await ui.text(panel), /Supervision status is unavailable/)
  })

  await step('Back up now writes a verified copy and lists it', async () => {
    await ui.go('Settings')
    // The path and the folder load after the page shows and push the button down: a click
    // aimed before they arrive lands where the button was.
    await ui.see(database, "//section[@aria-labelledby='database-file-heading']")
    await ui.see('No files in the backups folder yet.')
    await ui.click(button('Back up now'))
    const status = "//*[@role='status' and @aria-label='Backup']"
    await ui.see('Backed up at', status)
    await ui.see(`quick check said “ok”, schema version ${schemaVersion}`, status)
    const folder = join(dataDirectory, 'backups')
    const files = await readdir(folder)
    assert.equal(files.length, 1, `one file in backups/, not ${files.join(', ')}`)
    const name = files[0]!
    assert.match(name, /^inventory-\d{8}T\d{6}\.\d{3}Z\.sqlite3$/)
    await ui.see(join(folder, name), status)
    const listed = "//ul[@aria-label='Files in the backups folder']"
    await ui.see(name, listed)
    // The folder is read again once the backup is named. A .partial that an antivirus still
    // held open while it scanned it would be listed too, as incomplete.
    assert.doesNotMatch(await ui.text(listed), /Incomplete/, 'the window lists no incomplete copy')
    const copy = new DatabaseSync(join(folder, name), { readOnly: true })
    try {
      const value = (sql: string) => Object.values(copy.prepare(sql).get()!)[0]
      assert.equal(value('PRAGMA quick_check'), 'ok')
      assert.equal(value('PRAGMA user_version'), schemaVersion)
      assert.equal(value('PRAGMA journal_mode'), 'delete')
      assert.equal(value('SELECT count(*) FROM applications'), 1)
      assert.equal(value('SELECT count(*) FROM components'), 1)
      assert.equal(value('SELECT count(*) FROM resources'), 1)
      assert.equal(value('SELECT count(*) FROM http_checks'), 1)
      assert.equal(
        value("SELECT kind || ' ' || target FROM system_references WHERE machine_id IS NULL"),
        'repository https://git.example/status-page',
      )
      assert.equal(value("SELECT count(*) FROM observations WHERE source = 'manual'"), 1)
      assert.equal(value("SELECT count(*) FROM observations WHERE source = 'http'"), 2)
      // What the person changed in the window is logged with it, and the walk's inventory was
      // created in this schema, so the log is complete.
      assert.equal(
        value(
          "SELECT group_concat(action, ' ') FROM (SELECT action FROM change_log WHERE source = 'manual' ORDER BY rowid)",
        ),
        'created created check_saved',
      )
      assert.equal(value('SELECT count(*) FROM change_log_start'), 0)
    } finally {
      copy.close()
    }
  })

  const gate = agentGate(binary, database, env)

  await step('An agent adds an application through the gate and the window shows it', async () => {
    await ui.go('Settings')
    const access = "//section[@aria-labelledby='agent-gate-heading']"
    await ui.click(
      `${access}//label[.//span[normalize-space()='Read and write']]//input[@type='radio']`,
    )
    await ui.see('It applies at once', access)
    await ui.go('Applications')
    await ui.see('Status page')

    await gate.request('initialize', {
      protocolVersion: '2025-06-18',
      capabilities: {},
      clientInfo: { name: 'native-walk', version: '1' },
    })
    const status = await gate.tool('status')
    assert.equal(status.access, 'write')
    assert.equal(status.windowOpen, true, 'the gate sees the open window')
    const saved = await gate.tool('save_application', {
      name: 'Release notes',
      lifecycle: 'development',
      components: [{ name: 'site', kind: 'frontend', runsOn: 'Build server' }],
    })
    assert.equal((saved.change as { agent: string }).agent, 'native-walk')
    // Nothing in the window is touched: it notices the gate's commit on its own.
    await ui.see('1 component · 1 linked resource', "//main//a[@aria-label='Open Release notes']")
    await ui.go('Activity')
    await ui.see(
      'Added application «Release notes» (development) with site (frontend, on Build server)',
    )
    await ui.see('Agent · native-walk')
  })

  const copy = hostileCopy(copies, 'https://git.example/status-page')
  const workstation = 'Walk workstation'
  const localCopies = "//section[@aria-labelledby='local-copies-heading']"

  await step('Choose this computer in Settings', async () => {
    await ui.go('Infrastructure')
    await ui.click(button('New resource', '//main//header'))
    const resourceForm = form('New resource')
    await ui.fill(field('Name', resourceForm), workstation)
    await ui.choose(field('Kind', resourceForm), 'Workstation')
    await ui.click(button('Save resource', resourceForm))
    await ui.see(workstation, region(workstation))
    await ui.go('Settings')
    const section = "//section[@aria-labelledby='this-computer-heading']"
    // Nothing chosen yet: DevHQ says so instead of guessing from the paths that exist.
    await ui.see('No workstation has been chosen as this computer yet', section)
    await ui.choose(field('Workstation', section), workstation)
    await ui.click(button('Save', section))
    await ui.see(`This computer is ${workstation}`, section)
    const status = await gate.tool('status')
    assert.equal(
      (status.thisWorkstation as { problem: string | null }).problem,
      null,
      'the gate reads the same choice',
    )
  })

  await step('Declare a hostile working copy of the application', async () => {
    await ui.go('Applications')
    await ui.click("//main//a[@aria-label='Open Status page']")
    await ui.click(button('Edit application', '//main//header'))
    const applicationForm = form('Edit application')
    await ui.click(button('Add reference', applicationForm))
    // The second reference: the repository is the first.
    const second = (xpath: string) => `(${xpath})[2]`
    await ui.choose(second(field('Reference kind', applicationForm)), 'Local copy')
    await ui.fill(second(field('URL or path', applicationForm)), copy.path)
    await ui.choose(second(field('Machine', applicationForm)), `${workstation} · workstation`)
    await ui.click(button('Save application', applicationForm))
    await ui.see('Never read: DevHQ knows nothing of what this copy holds.')
    copy.assertUntouched('Declaring the copy')
  })

  await step('Read the copy in the window without running anything it names', async () => {
    await ui.click(`//button[@aria-label=${literal(`Read ${copy.path}`)}]`)
    await ui.see('Read and kept.')
    // What libgit2, confined, found: the commit was never pushed (the remote was never fetched),
    // and the file under a filter DevHQ does not run is neither changed nor clean.
    await ui.see('1 commit of HEAD on no remote branch')
    await ui.see('1 changed file · 1 new entry · 1 file DevHQ could not verify')
    await ui.see('A remote pointed to the repository Status page declares.')
    copy.assertUntouched('Read')
  })

  await step('Read all and an agent read it again, still running nothing', async () => {
    await ui.go('Overview')
    await ui.see('1 copy on this computer', localCopies)
    await ui.see('Work that existed only in a copy (1)', localCopies)
    await ui.click(button('Read all on this computer', localCopies))
    await ui.see('Read 1 copy.', localCopies)
    copy.assertUntouched('Read all')
    const read = await gate.tool('read_copies')
    assert.deepEqual(read.failures, [])
    const [reading] = read.readings as { agent: string; outcome: string; changed: number }[]
    assert.deepEqual(
      [reading!.agent, reading!.outcome, reading!.changed],
      ['native-walk', 'read', 1],
    )
    // The window notices the gate's commit on its own.
    await ui.see('asked by native-walk', localCopies)
    copy.assertUntouched("The gate's read")
    // The installed Git, on the same copy, runs what its config names: the copy is hostile.
    assert.match(copy.gitStatusMarks(), /fsmonitor/, 'git status ran the fsmonitor it names')
  })

  await step('Closing the window ends DevHQ and leaves one complete file', async () => {
    closeWindow(pid!, env)
    await until('DevHQ to exit', () => !alive(pid!), 20_000)
    await session!.end()
    session = undefined
    // The last connection folded the log back into the file, which stays in WAL mode.
    assert.equal(existsSync(`${database}-wal`), false, 'no write-ahead log is left behind')
    assert.equal(journalOf(database), 'wal')
    // The system releases the window's lock with its process. Windows does it "as resources
    // allow", which can be a moment after the process is gone: the first Windows run, under
    // real-time protection, still saw the lock held right after DevHQ exited. The window lock's
    // own test allows the same 10 s.
    const exited = Date.now()
    await until(
      'the gate to see no window',
      async () => (await gate.tool('status')).windowOpen === false,
      10_000,
    )
    console.log(`  the gate saw no window ${Date.now() - exited} ms after DevHQ exited`)
    assert.equal(await gate.close(), 0, 'the gate ends when its input closes')
    // Three readings were kept, on the chosen workstation, one asked for by the agent.
    const inventory = new DatabaseSync(database, { readOnly: true })
    try {
      const rows = inventory
        .prepare(
          'SELECT r.agent, r.outcome, r.path FROM copy_readings r JOIN resources m ON m.id = r.machine_id WHERE m.name = ? ORDER BY r.observed_at',
        )
        .all(workstation)
      assert.deepEqual(
        rows.map((row) => [row.agent, row.outcome, row.path]),
        [
          [null, 'read', copy.path],
          [null, 'read', copy.path],
          ['native-walk', 'read', copy.path],
        ],
      )
    } finally {
      inventory.close()
    }
    // The v6 inventory DevHQ refused at the start still holds exactly what it did.
    assert.ok(readFileSync(aside).equals(older), 'the v6 file changed')
  })
} catch (error) {
  failed = true
  console.error(error)
} finally {
  await session?.end()
  // A failed walk can leave DevHQ running; it must be gone before its folder is removed.
  if (pid !== undefined) await kill(pid)
  server.close()
  await stopAll(helpers)
  await isolated.cleanup()
  if (!process.env.DEVHQ_E2E_KEEP_DATA)
    await rm(copies, { recursive: true, force: true, maxRetries: 5 })
}
process.exit(failed ? 1 : 0)
