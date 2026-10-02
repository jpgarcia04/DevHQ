/**
 * The native end-to-end walk: the real DevHQ, with its Rust process, its SQLite file and the
 * system WebView, driven through tauri-driver like a person would use it. Everything else
 * (Vitest, the Playwright preview, the Rust tests on Tauri's mock runtime) stops short of the
 * real window; this walk is the one test that crosses it.
 *
 * It starts from a schema v6 inventory in rollback-journal mode, as DevHQ files were before
 * write-ahead logging, in a data directory of its own (see `isolate`). It never touches a
 * real inventory and it talks to no server but its own, on 127.0.0.1.
 *
 * Run it with `npm run e2e:native`, which builds the debug app first. DEVHQ_APP points it at
 * another build. Screenshots of every step land in test-results/native/.
 */
import assert from 'node:assert/strict'
import type { ChildProcess } from 'node:child_process'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { mkdir, readdir, rm, writeFile } from 'node:fs/promises'
import { createServer, type RequestListener } from 'node:http'
import { createServer as createHttpsServer } from 'node:https'
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
} from './app.ts'
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
 * write-ahead logging: the walk checks that the real app converts it.
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

const helpers: ChildProcess[] = []
const isolated = await isolate()
const server = await healthServer()
let session: Session | undefined
let pid: number | undefined
let failed = false
let count = 0

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
  await mkdir(dataDirectory, { recursive: true })
  seedVersion6(database)
  assert.equal(journalOf(database), 'rollback')
  await display(env, helpers)
  console.log(`DevHQ: ${binary}\nData: ${dataDirectory}`)
  ;({ session, pid } = await launch(binary, env, helpers))
  const ui = window(session)
  const resource = region('Build server')

  await step('Mi inventario opens on the v6 inventory', async () => {
    await ui.see('Nothing is registered yet.')
    await ui.find("//button[@aria-pressed='true' and normalize-space()='My inventory']")
    assert.ok(alive(pid!), 'DevHQ is running')
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
    await ui.see(name, "//ul[@aria-label='Files in the backups folder']")
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
      assert.equal(value("SELECT count(*) FROM observations WHERE source = 'manual'"), 1)
      assert.equal(value("SELECT count(*) FROM observations WHERE source = 'http'"), 2)
    } finally {
      copy.close()
    }
  })

  await step('Closing the window ends DevHQ and leaves one complete file', async () => {
    closeWindow(pid!, env)
    await until('DevHQ to exit', () => !alive(pid!), 20_000)
    await session!.end()
    session = undefined
    // The last connection folded the log back into the file, which stays in WAL mode.
    assert.equal(existsSync(`${database}-wal`), false, 'no write-ahead log is left behind')
    assert.equal(journalOf(database), 'wal')
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
}
process.exit(failed ? 1 : 0)
