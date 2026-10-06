import type {
  EntityRef,
  HttpEvidence,
  Observation,
  ObservationResult,
  RecordedEvent,
  RecordedIncident,
} from '@/domain/models'
import type {
  BackupFile,
  HttpCheck,
  InventoryApplication,
  InventoryResource,
} from '@/features/inventory/inventory-api'

/**
 * What the preview's simulated native process holds: every record, as SQLite would, not just
 * what one snapshot carries. Observations and events are in the order they were recorded, which
 * breaks ties in time as the rowid does.
 */
export interface PreviewDatabase {
  applications: InventoryApplication[]
  resources: InventoryResource[]
  observations: Observation[]
  incidents: RecordedIncident[]
  events: RecordedEvent[]
  httpChecks: HttpCheck[]
  /** The files in the backups folder, newest first. */
  backups: BackupFile[]
  /** Reading fails, as it would with an unreadable database. */
  unavailable: boolean
}

/**
 * - `history`: a small inventory in every evidence state, with a supervised resource whose
 *   automated results go back further than a snapshot carries.
 * - `empty`: nothing registered yet.
 * - `unavailable`: the inventory cannot be read.
 */
export const scenarios = ['history', 'empty', 'unavailable'] as const
export type Scenario = (typeof scenarios)[number]

const minute = 60_000
const at = (now: number, minutesAgo: number) => new Date(now - minutesAgo * minute).toISOString()

const web: EntityRef = { type: 'resource', id: 'web-host' }
const db: EntityRef = { type: 'resource', id: 'db-host' }
const shop: EntityRef = { type: 'application', id: 'shop' }
const blog: EntityRef = { type: 'application', id: 'blog' }

const resultTitles: Record<ObservationResult, [string, RecordedEvent['outcome']]> = {
  healthy: ['Healthy', 'success'],
  degraded: ['Attention required', 'warning'],
  unhealthy: ['Unhealthy', 'failure'],
}

function nothing(): PreviewDatabase {
  return {
    applications: [],
    resources: [],
    observations: [],
    incidents: [],
    events: [],
    httpChecks: [],
    backups: [],
    unavailable: false,
  }
}

/** The name a backup taken at `time` gets: its UTC moment in ISO 8601's basic format. */
const backupName = (time: string) => `inventory-${time.replace(/[-:]/g, '')}.sqlite3`

/** Two backups and the `.partial` an interrupted one left behind. */
function sampleBackups(now: number): BackupFile[] {
  const file = (minutesAgo: number, sizeBytes: number, partial = false): BackupFile => ({
    name: backupName(at(now, minutesAgo)) + (partial ? '.partial' : ''),
    sizeBytes,
    modifiedAt: at(now, minutesAgo),
    partial,
    storedAt: null,
  })
  return [file(90, 1_843_200), file(3 * 24 * 60, 655_360, true), file(9 * 24 * 60, 1_572_864)]
}

/** Sample records relative to `now`, the preview's clock, so validity reads as it would. */
export function sampleDatabase(scenario: Scenario, now: number): PreviewDatabase {
  if (scenario === 'empty') return nothing()
  // The backups folder can be read even when the inventory cannot.
  if (scenario === 'unavailable')
    return { ...nothing(), backups: sampleBackups(now), unavailable: true }
  const time = (minutesAgo: number) => at(now, minutesAgo)
  const observations: Observation[] = []
  const events: RecordedEvent[] = []
  // Every sample record was entered when it happened.
  const event = (item: Omit<RecordedEvent, 'recordedAt'>) =>
    events.push({ ...item, recordedAt: item.occurredAt })

  const observe = (
    id: string,
    subject: EntityRef,
    minutesAgo: number,
    result: ObservationResult,
    summary: string,
    validForMinutes: number,
    http: HttpEvidence | null = null,
  ) => {
    const observedAt = time(minutesAgo)
    observations.push({
      id,
      subject,
      result,
      summary,
      source: http ? 'http' : 'manual',
      observedAt,
      validUntil: time(minutesAgo - validForMinutes),
      recordedAt: observedAt,
      http,
      agent: null,
    })
    const [label, outcome] = resultTitles[result]
    event({
      id: `evt-${id}`,
      subject,
      kind: 'observation',
      outcome,
      title: `${http ? 'HTTP check' : 'Observation'}: ${label}`,
      detail: summary,
      source: http ? 'http' : 'manual',
      observationId: id,
      incidentId: null,
      occurredAt: observedAt,
      agent: null,
    })
  }

  // Web host: supervised every 15 minutes for longer than a snapshot's 200 results, with a
  // few failures that say different things.
  for (let run = 259; run >= 0; run -= 1) {
    const minutesAgo = 2 + run * 15
    const failing = run >= 3 && run <= 5
    const timedOut = run === 40
    const latencyMs = timedOut ? 5002 : failing ? 41 : 80 + (run % 9)
    const status = timedOut ? null : failing ? 503 : 200
    const summary = timedOut
      ? `Timed out after ${latencyMs} ms`
      : failing
        ? `HTTP 503, expected 200, in ${latencyMs} ms`
        : `HTTP 200 as expected, in ${latencyMs} ms`
    observe(`web-${run}`, web, minutesAgo, status === 200 ? 'healthy' : 'unhealthy', summary, 60, {
      url: 'https://shop.example/health',
      expectedStatus: 200,
      status,
      latencyMs,
      error: timedOut ? `Timed out after ${latencyMs} ms` : null,
      redirects: [],
    })
  }
  // Recorded among the automated results, in time order, as SQLite would have them.
  observe('web-tls', web, 80, 'healthy', 'TLS certificate checked by hand: valid until March', 240)
  observe('db-backups', db, 2900, 'healthy', 'Backups restored to a scratch database', 1440)
  observe('db-disk', db, 50, 'degraded', 'Disk at 86% on /var/lib/postgresql', 240)
  observe('shop-manual', shop, 7200, 'healthy', 'Checkout tested by hand end to end', 1440)
  observe('shop-run', shop, 120, 'healthy', 'HTTP 200 as expected, in 134 ms', 60, {
    url: 'https://shop.example/ready',
    expectedStatus: 200,
    status: 200,
    latencyMs: 134,
    error: null,
    redirects: [{ status: 301, location: 'https://shop.example/ready/' }],
  })

  const incidents: RecordedIncident[] = [
    {
      id: 'inc-disk',
      subject: db,
      title: 'Disk almost full',
      description: 'The Postgres data volume is above 85%.',
      severity: 'warning',
      status: 'investigating',
      source: 'manual',
      openedAt: time(45),
      updatedAt: time(30),
      resolvedAt: null,
      agent: null,
    },
    {
      id: 'inc-checkout',
      subject: shop,
      title: 'Checkout returning 503',
      description: 'Payments failed while the API was restarting.',
      severity: 'critical',
      status: 'resolved',
      source: 'manual',
      openedAt: time(75),
      updatedAt: time(40),
      resolvedAt: time(40),
      agent: null,
    },
  ]
  const incidentEvent = (
    id: string,
    incident: RecordedIncident,
    minutesAgo: number,
    title: string,
    outcome: RecordedEvent['outcome'],
    detail: string,
  ) =>
    event({
      id,
      subject: incident.subject,
      kind: 'incident',
      outcome,
      title: `${title}: ${incident.title}`,
      detail,
      source: 'manual',
      observationId: null,
      incidentId: incident.id,
      occurredAt: time(minutesAgo),
      agent: null,
    })
  incidentEvent('evt-checkout-open', incidents[1]!, 75, 'Incident opened', 'failure', '')
  incidentEvent(
    'evt-checkout-done',
    incidents[1]!,
    40,
    'Incident resolved',
    'success',
    'Rolled back',
  )
  incidentEvent('evt-disk-open', incidents[0]!, 45, 'Incident opened', 'warning', '')
  incidentEvent(
    'evt-disk-look',
    incidents[0]!,
    30,
    'Incident under investigation',
    'info',
    'Rotating old WAL files',
  )
  const logged = (
    id: string,
    subject: EntityRef,
    kind: RecordedEvent['kind'],
    minutesAgo: number,
    title: string,
    detail: string,
    agent: string | null = null,
  ) =>
    event({
      id,
      subject,
      kind,
      outcome: kind === 'note' ? 'info' : 'success',
      title,
      detail,
      source: agent ? 'agent' : 'manual',
      observationId: null,
      incidentId: null,
      occurredAt: time(minutesAgo),
      agent,
    })
  logged('evt-deploy', shop, 'deployment', 70, 'Deployed 2.4.1', 'Checkout fixes')
  // Noted by an agent the person asked through the agent gate.
  logged(
    'evt-agent-note',
    shop,
    'note',
    12,
    'Checkout latency back under 300 ms',
    'Compared the last three HTTP results after the 2.4.1 deployment',
    'claude-code',
  )
  logged(
    'evt-backup',
    db,
    'backup',
    540,
    'Nightly backup verified',
    'Restored and compared row counts',
  )
  logged('evt-theme', blog, 'note', 2880, 'Draft theme in review', '')
  logged('evt-kernel', web, 'maintenance', 4320, 'Kernel update and reboot', 'Ubuntu 24.04.3')

  const byRecording = <T extends { recordedAt: string }>(items: T[]) =>
    items.sort((a, b) => Date.parse(a.recordedAt) - Date.parse(b.recordedAt))
  return {
    applications: [
      {
        id: 'shop',
        name: 'Shop',
        description: 'Online store and checkout',
        lifecycle: 'production',
        version: '2.4.1',
        components: [
          { id: 'shop-api', name: 'API', kind: 'backend', resourceId: 'web-host' },
          { id: 'shop-front', name: 'Storefront', kind: 'frontend', resourceId: 'web-host' },
          { id: 'shop-orders', name: 'Orders', kind: 'database', resourceId: 'db-host' },
          { id: 'shop-jobs', name: 'Mailer', kind: 'docker', resourceId: null },
        ],
      },
      {
        id: 'blog',
        name: 'Blog',
        description: 'Personal blog, still in development',
        lifecycle: 'development',
        version: null,
        components: [{ id: 'blog-site', name: 'Site', kind: 'frontend', resourceId: 'web-host' }],
      },
    ],
    resources: [
      {
        id: 'db-host',
        name: 'Database host',
        kind: 'server',
        description: 'Postgres 17',
        operatingSystem: 'Debian 13',
      },
      {
        id: 'laptop',
        name: 'Dev laptop',
        kind: 'workstation',
        description: '',
        operatingSystem: 'Windows 11',
      },
      {
        id: 'web-host',
        name: 'Web host',
        kind: 'server',
        description: 'Nginx and the API',
        operatingSystem: 'Ubuntu 24.04',
      },
    ],
    observations: byRecording(observations),
    incidents,
    events: byRecording(events),
    httpChecks: [
      {
        subject: web,
        url: 'https://shop.example/health',
        expectedStatus: 200,
        timeoutSeconds: 5,
        validForMinutes: 60,
        updatedAt: time(10_000),
        monitor: 'active',
        monitorIntervalMinutes: 15,
      },
      {
        subject: shop,
        url: 'https://shop.example/ready',
        expectedStatus: 200,
        timeoutSeconds: 5,
        validForMinutes: 60,
        updatedAt: time(9_000),
        monitor: 'off',
        monitorIntervalMinutes: null,
      },
    ],
    backups: sampleBackups(now),
    unavailable: false,
  }
}
