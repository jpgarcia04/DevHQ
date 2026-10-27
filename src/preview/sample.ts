import type {
  EntityRef,
  HttpEvidence,
  Observation,
  ObservationResult,
  RecordedEvent,
  RecordedIncident,
} from '@/domain/models'
import type {
  ConfigurationChange,
  BackupFile,
  CopyReading,
  HttpCheck,
  InventoryApplication,
  InventoryResource,
  ThisWorkstation,
  UpgradeInspection,
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
  changes: ConfigurationChange[]
  changesSince: string | null
  /** Every reading of a local copy, in the order they were recorded. */
  copyReadings: CopyReading[]
  /** Which workstation this computer is, from the local association beside the inventory. */
  thisWorkstation: ThisWorkstation
  /** The files in the backups folder, newest first. */
  backups: BackupFile[]
  /** Why every read is refused, as the native process refuses a database it cannot or will not
   * read; null when it reads. */
  unavailable: { code: string; message: string } | null
  /** What the window finds when it looks at an inventory it refused as outdated. */
  upgrade: UpgradeInspection | null
}

/** Where the native process would keep this computer's association, on Windows. */
const associationFile = 'C:\\Users\\you\\AppData\\Local\\com.devhq.desktop\\this-workstation.json'
/** Where the native process would keep the inventory, on Windows. */
const previewDatabasePath = 'C:\\Users\\you\\AppData\\Roaming\\com.devhq.desktop\\inventory.sqlite3'

const notChosen: ThisWorkstation = {
  resourceId: null,
  chosenAt: null,
  problem: 'not_chosen',
  message:
    'No workstation has been chosen as this computer yet: choose one in Settings › This computer.',
  file: associationFile,
  storedAt: null,
}

/**
 * - `history`: a small inventory in every evidence state, with a supervised resource whose
 *   automated results go back further than a snapshot carries.
 * - `empty`: nothing registered yet.
 * - `unavailable`: the inventory cannot be read.
 * - `outdated`: the inventory is in an older schema, which DevHQ serves nothing from until the
 *   person backs it up and upgrades it, which the preview, like every write, refuses.
 */
export const scenarios = ['history', 'empty', 'unavailable', 'outdated'] as const
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
    changes: [],
    changesSince: null,
    copyReadings: [],
    thisWorkstation: notChosen,
    backups: [],
    unavailable: null,
    upgrade: null,
  }
}

/** Readings of the laptop's dotfiles: one every 6 hours, by an agent, more than a page. */
export const dotfilesReadings = 205

/**
 * Readings of the sample's local copies, in the order they were recorded:
 * - the Shop's clone on the laptop: five days ago, before it was cloned there (not a copy); three
 *   days ago, clean and published; two hours ago, with a commit on no remote, a stash and changes.
 *   Blog declares the same clone, never read.
 * - Blog's clone on the desktop, another computer, read there yesterday: a detached HEAD in a
 *   rebase left halfway, more than 1000 commits on no remote, a working tree that took too long,
 *   and a remote that is not the repository Blog declares.
 * - the laptop's dotfiles, read every 6 hours at a path it has since moved from, and not found
 *   there the last time: more readings than a page holds.
 * - the archived Old site's clone, with a stash: kept, but out of Read all and of Overview.
 */
function sampleReadings(now: number): CopyReading[] {
  const reading = (id: string, minutesAgo: number, facts: Partial<CopyReading>): CopyReading => ({
    id,
    subject: shop,
    referenceId: 'shop-clone',
    machineId: 'laptop',
    path: 'C:\\Users\\you\\src\\web',
    source: 'read',
    reader: 'libgit2 1.9.7',
    agent: null,
    observedAt: at(now, minutesAgo),
    recordedAt: at(now, minutesAgo),
    outcome: 'read',
    detail: null,
    branch: 'main',
    head: '4f2a9c1d8e7b6a5f4e3d2c1b0a9f8e7d6c5b4a39',
    lastCommitAt: at(now, minutesAgo + 300),
    unpublishedOnHead: 0,
    unpublishedElsewhere: 0,
    stashes: 0,
    syncedAt: at(now, minutesAgo + 30),
    remotes: [{ name: 'origin', url: 'git@github.com:example/web.git' }],
    operation: null,
    changed: 0,
    untracked: 0,
    unverifiable: 0,
    worktreeUnknown: null,
    ...facts,
  })
  const notRead = {
    branch: null,
    head: null,
    lastCommitAt: null,
    unpublishedOnHead: null,
    unpublishedElsewhere: null,
    stashes: null,
    syncedAt: null,
    remotes: null,
    changed: null,
    untracked: null,
    unverifiable: null,
  }
  const dotfiles = (index: number) =>
    reading(`dotfiles-${index}`, (dotfilesReadings - index) * 6 * 60 + 45, {
      subject: { type: 'resource', id: 'laptop' },
      referenceId: 'laptop-dotfiles',
      path: 'C:\\Users\\you\\dotfiles',
      agent: 'claude-code',
      head: 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678',
      lastCommitAt: at(now, 40 * 24 * 60),
      syncedAt: at(now, 40 * 24 * 60),
      remotes: [{ name: 'origin', url: 'https://github.com/you/dotfiles' }],
      ...(index === dotfilesReadings - 1
        ? { ...notRead, outcome: 'missing' as const, detail: 'Nothing is at this path.' }
        : {}),
    })
  return [
    reading('reading-gone', 5 * 24 * 60, {
      ...notRead,
      outcome: 'not_a_copy',
      detail:
        'No repository starts at this folder. DevHQ does not look in the folders above it: declare the folder at the root of the copy.',
    }),
    ...Array.from({ length: dotfilesReadings - 1 }, (_, index) => dotfiles(index)),
    reading('old-site-read', 4 * 24 * 60, {
      subject: { type: 'application', id: 'old-site' },
      referenceId: 'old-site-clone',
      path: 'C:\\Users\\you\\src\\old-site',
      head: '0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c',
      lastCommitAt: at(now, 400 * 24 * 60),
      syncedAt: at(now, 400 * 24 * 60),
      stashes: 2,
      remotes: [{ name: 'origin', url: 'https://github.com/example/old-site.git' }],
    }),
    reading('reading-old', 3 * 24 * 60, {}),
    reading('blog-desktop-read', 26 * 60, {
      subject: blog,
      referenceId: 'blog-desktop',
      machineId: 'desktop',
      path: '/home/you/src/blog',
      branch: null,
      head: '7e6d5c4b3a291807f6e5d4c3b2a1908f7e6d5c4b',
      lastCommitAt: at(now, 30 * 60),
      unpublishedElsewhere: 1001,
      syncedAt: null,
      remotes: [
        { name: 'origin', url: 'git@gitlab.com:you/blog.git' },
        { name: 'deploy', url: null },
      ],
      operation: 'rebase',
      changed: null,
      untracked: null,
      unverifiable: null,
      worktreeUnknown: 'Reading the working tree took longer than 30 s, so DevHQ stopped it.',
    }),
    dotfiles(dotfilesReadings - 1),
    reading('reading-new', 120, {
      head: '9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a07',
      lastCommitAt: at(now, 140),
      unpublishedOnHead: 1,
      stashes: 1,
      changed: 2,
      untracked: 1,
      agent: 'claude-code',
    }),
  ]
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
    return {
      ...nothing(),
      backups: sampleBackups(now),
      unavailable: {
        code: 'database_error',
        message: 'Inventory database error: unable to open database file',
      },
    }
  if (scenario === 'outdated')
    return {
      ...nothing(),
      backups: sampleBackups(now),
      unavailable: {
        code: 'schema_outdated',
        message: `${previewDatabasePath}: A DevHQ inventory that declares schema version 6, older than this DevHQ's (13). This DevHQ never upgrades an inventory when it opens it: neither its window nor the gate serves anything from this one, and both leave it as it is. Only the person upgrades it, from DevHQ's window (Back up and upgrade), which first makes and verifies a backup of it; the gate never does. Nothing was read or changed.`,
      },
      upgrade: {
        database: previewDatabasePath,
        storedAt: null,
        state: 'outdated',
        storedVersion: 6,
        version: 13,
        message: `A DevHQ inventory in schema version 6, older than this DevHQ's (13). DevHQ read it without changing it, and serves nothing from it until it is upgraded.`,
        upgradable: true,
        problems: [],
        tables: [
          { table: 'applications', rows: 2 },
          { table: 'components', rows: 4 },
          { table: 'events', rows: 31 },
          { table: 'http_checks', rows: 1 },
          { table: 'incidents', rows: 1 },
          { table: 'observations', rows: 27 },
          { table: 'resources', rows: 3 },
        ],
        fingerprint: '3a7bd3e2360a3d29eea436fcfb7e44c735d117c42d1c1835420b6b9942dd4f1b',
        backups: previewDatabasePath.replace(/inventory\.sqlite3$/, 'backups'),
      },
    }
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
        // Shop and Blog are parts of one monorepo, cloned on the laptop.
        references: [
          {
            id: 'shop-repo',
            kind: 'repository',
            target: 'https://github.com/example/web',
            label: null,
            machineId: null,
          },
          {
            id: 'shop-clone',
            kind: 'local_copy',
            target: 'C:\\Users\\you\\src\\web',
            label: null,
            machineId: 'laptop',
          },
          {
            id: 'shop-runbook',
            kind: 'documentation',
            target: 'https://docs.example/shop/runbook',
            label: 'Runbook',
            machineId: null,
          },
          {
            id: 'shop-logs',
            kind: 'logs',
            target: '/var/log/shop/api.log',
            label: 'API',
            machineId: 'web-host',
          },
          {
            id: 'shop-dashboard',
            kind: 'dashboard',
            target: 'https://grafana.example/d/shop',
            label: null,
            machineId: null,
          },
        ],
      },
      {
        id: 'blog',
        name: 'Blog',
        description: 'Personal blog, still in development',
        lifecycle: 'development',
        version: null,
        components: [{ id: 'blog-site', name: 'Site', kind: 'frontend', resourceId: 'web-host' }],
        references: [
          {
            id: 'blog-repo',
            kind: 'repository',
            target: 'https://github.com/example/web',
            label: null,
            machineId: null,
          },
          {
            id: 'blog-clone',
            kind: 'local_copy',
            target: 'C:\\Users\\you\\src\\web',
            label: null,
            machineId: 'laptop',
          },
          {
            id: 'blog-desktop',
            kind: 'local_copy',
            target: '/home/you/src/blog',
            label: null,
            machineId: 'desktop',
          },
        ],
      },
      {
        id: 'old-site',
        name: 'Old site',
        description: 'Replaced by Blog',
        lifecycle: 'archived',
        version: null,
        components: [],
        references: [
          {
            id: 'old-site-clone',
            kind: 'local_copy',
            target: 'C:\\Users\\you\\src\\old-site',
            label: null,
            machineId: 'laptop',
          },
        ],
      },
    ],
    resources: [
      {
        id: 'db-host',
        name: 'Database host',
        kind: 'server',
        description: 'Postgres 17',
        operatingSystem: 'Debian 13',
        references: [],
      },
      {
        id: 'desktop',
        name: 'Home desktop',
        kind: 'workstation',
        description: '',
        operatingSystem: 'Fedora 44',
        references: [],
      },
      {
        id: 'laptop',
        name: 'Dev laptop',
        kind: 'workstation',
        description: '',
        operatingSystem: 'Windows 11',
        // Its dotfiles, moved since they were last read.
        references: [
          {
            id: 'laptop-dotfiles',
            kind: 'local_copy',
            target: 'C:\\Users\\you\\src\\dotfiles',
            label: 'Dotfiles',
            machineId: 'laptop',
          },
        ],
      },
      {
        id: 'web-host',
        name: 'Web host',
        kind: 'server',
        description: 'Nginx and the API',
        operatingSystem: 'Ubuntu 24.04',
        // Its own logs are on it.
        references: [
          {
            id: 'web-logs',
            kind: 'logs',
            target: '/var/log/nginx',
            label: 'Nginx',
            machineId: 'web-host',
          },
        ],
      },
    ],
    observations: byRecording(observations),
    incidents,
    events: byRecording(events),
    // Changes to the configuration, by the person and by an agent through the agent gate: one
    // to a system since deleted. The log started when an upgrade found these systems in place.
    changes: [
      {
        id: 'change-check',
        subject: web,
        subjectName: 'Web host',
        source: 'manual',
        agent: null,
        action: 'check_saved',
        summary:
          'Changed the HTTP check of «Web host»: GET https://shop.example/health expecting 200 within 5 s, valid 60 min, supervised every 5 min while DevHQ is open',
        changes: [{ field: 'timeoutSeconds', before: 10, after: 5 }],
        occurredAt: time(30),
      },
      {
        id: 'change-version',
        subject: shop,
        subjectName: 'Shop',
        source: 'agent',
        agent: 'claude-code',
        action: 'updated',
        summary: 'Updated application «Shop»: version «2.4.0» → «2.4.1»',
        changes: [{ field: 'version', before: '2.4.0', after: '2.4.1' }],
        occurredAt: time(68),
      },
      {
        id: 'change-gone',
        subject: { type: 'resource', id: 'old-box' },
        subjectName: 'Old box',
        source: 'manual',
        agent: null,
        action: 'deleted',
        summary: 'Deleted resource «Old box», which had no history',
        changes: [
          { field: 'name', before: 'Old box', after: null },
          { field: 'kind', before: 'workstation', after: null },
        ],
        occurredAt: time(200),
      },
    ],
    changesSince: time(60 * 24 * 3),
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
    copyReadings: sampleReadings(now),
    // This computer is the laptop, chosen a week ago.
    thisWorkstation: {
      resourceId: 'laptop',
      chosenAt: time(7 * 24 * 60),
      problem: null,
      message: null,
      file: associationFile,
      storedAt: null,
    },
    backups: sampleBackups(now),
    unavailable: null,
    upgrade: null,
  }
}
