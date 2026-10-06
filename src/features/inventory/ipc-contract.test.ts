/// <reference types="vite/client" />
// The window's side of the IPC contract; the native side is src-tauri/src/ipc_contract.rs. Both
// check against the same JSON fixtures in contract/, so a name changed on one side breaks a test
// until the other side and the fixture agree again.
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks'
import { afterEach, describe, expect, it } from 'vitest'
import type { EntityRef } from '@/domain/models'
import { inventoryApi, inventoryError, type MonitorStatus } from './inventory-api'

/** The fixtures in contract/<kind>/, by file name without `.json`. */
function fixtures(files: Record<string, unknown>) {
  return Object.fromEntries(
    Object.entries(files).map(([file, value]) => [file.replace(/^.*\/(.+)\.json$/, '$1'), value]),
  )
}
const commandFixtures = fixtures(
  import.meta.glob('../../../contract/commands/*.json', { eager: true, import: 'default' }),
)
const responseFixtures = fixtures(
  import.meta.glob('../../../contract/responses/*.json', { eager: true, import: 'default' }),
)

const host: EntityRef = { type: 'resource', id: 'host' }
const shop: EntityRef = { type: 'application', id: 'shop' }

/**
 * One call of every `inventoryApi` method, with arguments typed as the window types them: the
 * compiler holds them to the window's types, the test holds what they send to the fixtures, and
 * Rust holds the fixtures to what its commands read. A new method needs its call here.
 */
const calls: Record<keyof typeof inventoryApi, () => Promise<unknown>> = {
  list: () => inventoryApi.list(),
  dataLocation: () => inventoryApi.dataLocation(),
  backupInventory: () => inventoryApi.backupInventory(),
  listBackups: () => inventoryApi.listBackups(),
  agentAccess: () => inventoryApi.agentAccess(),
  setAgentAccess: () => inventoryApi.setAgentAccess('read'),
  saveResource: () =>
    inventoryApi.saveResource({
      id: 'host',
      name: 'Host',
      kind: 'workstation',
      description: 'Rack 2',
      operatingSystem: 'Debian 13',
    }),
  saveApplication: () =>
    inventoryApi.saveApplication({
      id: 'shop',
      name: 'Shop',
      description: 'Store front',
      lifecycle: 'testing',
      version: '2.1',
      components: [
        { id: 'api', name: 'API', kind: 'backend', resourceId: 'host' },
        { name: 'Database', kind: 'database', resourceId: null },
      ],
    }),
  deleteApplication: () => inventoryApi.deleteApplication('draft'),
  deleteResource: () => inventoryApi.deleteResource('spare'),
  recordObservation: () =>
    inventoryApi.recordObservation({
      subject: host,
      result: 'degraded',
      summary: 'Disk at 85%',
      observedAt: '2026-01-01T09:30:00.000Z',
      validForMinutes: 60,
    }),
  olderObservations: () => inventoryApi.olderObservations(host, 'obs-http-2'),
  olderEvents: () => inventoryApi.olderEvents([shop, host], 'evt-http-2'),
  openIncident: () =>
    inventoryApi.openIncident({
      subject: shop,
      title: 'Checkout fails',
      description: 'Payments time out',
      severity: 'critical',
    }),
  updateIncidentStatus: () =>
    inventoryApi.updateIncidentStatus({
      id: 'incident-1',
      status: 'investigating',
      note: 'Reading the logs',
    }),
  recordEvent: () =>
    inventoryApi.recordEvent({
      subject: shop,
      kind: 'deployment',
      outcome: 'success',
      title: 'Deployed 2.1',
      detail: 'From the release branch',
      occurredAt: '2026-01-01T10:00:00.000Z',
    }),
  saveHttpCheck: () =>
    inventoryApi.saveHttpCheck({
      subject: shop,
      url: 'https://shop.example/health',
      expectedStatus: 204,
      timeoutSeconds: 5,
      validForMinutes: 30,
      monitorIntervalMinutes: 15,
    }),
  removeHttpCheck: () => inventoryApi.removeHttpCheck(host),
  setHttpCheckPaused: () => inventoryApi.setHttpCheckPaused(host, true),
  runHttpCheck: () => inventoryApi.runHttpCheck(host),
  watchHttpMonitor: () => inventoryApi.watchHttpMonitor(() => {}),
}

/** What a call sends to the native process: each command and its arguments, as JSON. */
async function sent(call: () => Promise<unknown>) {
  const messages: { command: string; args: unknown }[] = []
  mockIPC((command, args) => {
    // A channel crosses as `__CHANNEL__:<id>`, with an id the window picks at random.
    const json = JSON.stringify(args ?? {}).replace(/"__CHANNEL__:\d+"/g, '"__CHANNEL__:0"')
    messages.push({ command, args: JSON.parse(json) })
    return null
  })
  await call()
  return messages
}

describe('what inventoryApi sends to the native process', () => {
  afterEach(() => clearMocks())

  it.each(Object.entries(calls))(
    '%s sends exactly the arguments of its fixture',
    async (_, call) => {
      const messages = await sent(call)
      const command = messages[0]?.command ?? ''
      expect(commandFixtures, `contract/commands/${command}.json`).toHaveProperty(command)
      expect(messages).toStrictEqual([{ command, args: commandFixtures[command] }])
    },
  )

  it('sends a command for every fixture, and none without one', async () => {
    const commands: string[] = []
    for (const call of Object.values(calls))
      commands.push(...(await sent(call)).map((message) => message.command))
    expect(commands.sort()).toStrictEqual(Object.keys(commandFixtures).sort())
  })
})

type Answer<Method extends keyof typeof inventoryApi> = Awaited<
  ReturnType<(typeof inventoryApi)[Method]>
>

// What the window expects each command to answer, typed as `inventoryApi` declares it: the
// compiler holds these values to the window's types and the test holds them to the fixtures,
// which Rust holds to what it serializes.
const at = (time: string) => `2026-01-01T${time}:00Z`

const snapshot: Answer<'list'> = {
  applications: [
    {
      id: 'shop',
      name: 'Shop',
      description: 'Store front',
      lifecycle: 'production',
      version: '2.0',
      components: [
        { id: 'api', name: 'API', kind: 'backend', resourceId: 'host' },
        { id: 'db', name: 'Database', kind: 'database', resourceId: null },
      ],
    },
  ],
  resources: [
    {
      id: 'host',
      name: 'Host',
      kind: 'server',
      description: 'Rack 2',
      operatingSystem: 'Debian 13',
    },
  ],
  observations: [
    {
      id: 'obs-http',
      subject: host,
      result: 'healthy',
      summary: 'HTTP 200 as expected, in 42 ms',
      source: 'http',
      observedAt: at('10:05'),
      validUntil: at('10:20'),
      recordedAt: at('10:05'),
      http: {
        url: 'http://host.example/health',
        expectedStatus: 200,
        status: 200,
        latencyMs: 42,
        error: null,
        redirects: [{ status: 301, location: 'https://host.example/health' }],
      },
      agent: null,
    },
    {
      id: 'obs-manual',
      subject: shop,
      result: 'degraded',
      summary: 'Checkout is slow',
      source: 'manual',
      observedAt: at('09:30'),
      validUntil: at('10:30'),
      recordedAt: at('09:45'),
      http: null,
      agent: null,
    },
  ],
  incidents: [
    {
      id: 'incident-1',
      subject: shop,
      title: 'Checkout fails',
      description: 'Payments time out',
      severity: 'critical',
      status: 'resolved',
      source: 'manual',
      openedAt: at('09:00'),
      updatedAt: at('09:50'),
      resolvedAt: at('09:50'),
      agent: null,
    },
  ],
  events: [
    {
      id: 'evt-http',
      subject: host,
      kind: 'observation',
      outcome: 'success',
      title: 'HTTP check: Healthy',
      detail: 'HTTP 200 as expected, in 42 ms',
      source: 'http',
      observationId: 'obs-http',
      incidentId: null,
      occurredAt: at('10:05'),
      recordedAt: at('10:05'),
      agent: null,
    },
    {
      id: 'evt-incident',
      subject: shop,
      kind: 'incident',
      outcome: 'success',
      title: 'Incident resolved: Checkout fails',
      detail: 'Rolled back',
      source: 'manual',
      observationId: null,
      incidentId: 'incident-1',
      occurredAt: at('09:50'),
      recordedAt: at('09:50'),
      agent: null,
    },
    {
      id: 'evt-agent',
      subject: shop,
      kind: 'deployment',
      outcome: 'success',
      title: 'Deployed 2.4.1',
      detail: 'Noted while the person watched',
      source: 'agent',
      observationId: null,
      incidentId: null,
      occurredAt: at('09:40'),
      recordedAt: at('09:41'),
      agent: 'claude-code',
    },
  ],
  httpChecks: [
    {
      subject: host,
      url: 'http://host.example/health',
      expectedStatus: 200,
      timeoutSeconds: 5,
      validForMinutes: 15,
      updatedAt: at('08:00'),
      monitor: 'active',
      monitorIntervalMinutes: 5,
    },
  ],
  changes: [
    {
      id: 'change-2',
      subject: shop,
      subjectName: 'Shop',
      source: 'agent',
      agent: 'claude-code',
      action: 'updated',
      summary: 'Updated application «Shop»: version «1.9» → «2.0»',
      changes: [{ field: 'version', before: '1.9', after: '2.0' }],
      occurredAt: at('09:35'),
    },
    {
      id: 'change-1',
      subject: host,
      subjectName: 'Host',
      source: 'manual',
      agent: null,
      action: 'check_removed',
      summary:
        'Removed the HTTP check of «Host» (http://host.example/old); the evidence it recorded stays',
      changes: [{ field: 'url', before: 'http://host.example/old', after: null }],
      occurredAt: at('08:45'),
    },
    {
      id: 'change-0',
      subject: shop,
      subjectName: 'Shop',
      source: 'agent',
      agent: 'codex',
      action: 'created',
      summary: 'Added application «Shop» (development)',
      changes: null,
      occurredAt: at('08:40'),
    },
  ],
  changesSince: at('08:30'),
  automatedHistory: { limit: 200, truncated: [{ subject: host, since: at('10:05') }] },
}

const olderObservations: Answer<'olderObservations'> = {
  observations: [
    {
      id: 'obs-timeout',
      subject: host,
      result: 'unhealthy',
      summary: 'Timed out after 5 s',
      source: 'http',
      observedAt: at('10:00'),
      validUntil: at('10:15'),
      recordedAt: at('10:00'),
      http: {
        url: 'http://host.example/health',
        expectedStatus: 200,
        status: null,
        latencyMs: 5000,
        error: 'Timed out after 5 s',
        redirects: [],
      },
      agent: null,
    },
  ],
  more: true,
}

const olderEvents: Answer<'olderEvents'> = {
  events: [
    {
      id: 'evt-timeout',
      subject: host,
      kind: 'observation',
      outcome: 'failure',
      title: 'HTTP check: Unhealthy',
      detail: 'Timed out after 5 s',
      source: 'http',
      observationId: 'obs-timeout',
      incidentId: null,
      occurredAt: at('10:00'),
      recordedAt: at('10:00'),
      agent: null,
    },
  ],
  more: false,
}

const folder = 'C:\\Users\\Juan Pablo\\AppData\\Roaming\\com.devhq.desktop'
// Where Windows keeps that folder's files for a DevHQ started from a packaged app.
const packaged =
  'C:\\Users\\Juan Pablo\\AppData\\Local\\Packages\\Claude_pzs8sxrjxfjjc\\LocalCache\\Roaming\\com.devhq.desktop'
const dataLocation: Answer<'dataLocation'> = {
  database: `${folder}\\inventory.sqlite3`,
  directory: folder,
  storedAt: `${packaged}\\inventory.sqlite3`,
}

const agentAccess: Answer<'agentAccess'> = {
  mode: 'read',
  changedAt: at('09:15'),
  executable: 'C:\\Users\\Juan Pablo\\AppData\\Local\\DevHQ\\devhq.exe',
}

const backup: Answer<'backupInventory'> = {
  name: 'inventory-20260101T100500.123Z.sqlite3',
  path: `${folder}\\backups\\inventory-20260101T100500.123Z.sqlite3`,
  storedAt: null,
  takenAt: '2026-01-01T10:05:00.123Z',
  sizeBytes: 2_097_152,
  quickCheck: 'ok',
  schemaVersion: 6,
  counts: {
    applications: 1,
    components: 2,
    resources: 1,
    observations: 2,
    incidents: 1,
    events: 2,
    httpChecks: 1,
  },
}

const backups: Answer<'listBackups'> = {
  directory: `${folder}\\backups`,
  files: [
    {
      name: 'inventory-20260101T100500.123Z.sqlite3',
      sizeBytes: 2_097_152,
      modifiedAt: '2026-01-01T10:05:01Z',
      partial: false,
      storedAt: null,
    },
    {
      name: 'inventory-20260101T090000.000Z.sqlite3.partial',
      sizeBytes: 1_048_576,
      modifiedAt: null,
      partial: true,
      storedAt: `${packaged}\\backups\\inventory-20260101T090000.000Z.sqlite3.partial`,
    },
  ],
}

/** What the supervision channel of `watch_http_monitor` delivers. */
const supervision: MonitorStatus = {
  checks: [
    {
      subject: host,
      running: false,
      lastRunAt: at('10:05'),
      nextRunAt: at('10:10'),
      lastError: { message: 'Inventory database error: disk I/O error', at: at('09:55') },
    },
  ],
  error: null,
  recorded: 3,
  changes: 7,
}

const answers: Record<string, unknown> = {
  list_inventory: snapshot,
  older_observations: olderObservations,
  older_events: olderEvents,
  data_location: dataLocation,
  backup_inventory: backup,
  list_backups: backups,
  agent_access: agentAccess,
  watch_http_monitor: supervision,
}

describe('what the native process answers', () => {
  it.each(Object.entries(answers))('%s has the shape the window expects', (name, answer) => {
    expect(answer).toStrictEqual(responseFixtures[name])
  })

  it('reads the message of a refusal where Rust puts it', () => {
    expect(inventoryError(responseFixtures.error)).toBe('The resource ghost does not exist.')
  })

  it('expects an answer for every response fixture', () => {
    expect([...Object.keys(answers), 'error'].sort()).toStrictEqual(
      Object.keys(responseFixtures).sort(),
    )
  })
})
