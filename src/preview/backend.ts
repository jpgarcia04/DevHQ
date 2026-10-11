import type { Channel } from '@tauri-apps/api/core'
import { sameSubject, subjectsOf } from '@/domain/evidence'
import type { EntityRef, Observation, RecordedEvent } from '@/domain/models'
import type {
  AgentAccess,
  BackupFolder,
  CopyReading,
  DataLocation,
  EventPage,
  InventoryApplication,
  InventoryResource,
  InventorySnapshot,
  LocalCopy,
  MonitorStatus,
  ObservationPage,
  ReadingPage,
  SharedRepository,
  ThisWorkstation,
} from '@/features/inventory/inventory-api'
import type { PreviewDatabase } from './sample'

/** As in the native process: automated results per system in a snapshot, and per page. */
const limit = 200

/** Where the native process would keep the inventory, on Windows. */
const folder = 'C:\\Users\\you\\AppData\\Roaming\\com.devhq.desktop'

/** Answers every write: the preview has nothing to write to. */
export const readOnly = {
  code: 'preview_read_only',
  message: 'This preview shows sample data and has no database: nothing was saved.',
}

const invalid = (message: string) => ({ code: 'invalid_input', message })

/**
 * Newest first, as the native queries order them: by time, then by when it was recorded, then
 * by the order it was recorded in (the rowid).
 */
function newestFirst<T>(items: T[], time: (item: T) => string, recorded: (item: T) => string) {
  return items
    .map((item, order) => ({ item, order }))
    .sort(
      (a, b) =>
        Date.parse(time(b.item)) - Date.parse(time(a.item)) ||
        Date.parse(recorded(b.item)) - Date.parse(recorded(a.item)) ||
        b.order - a.order,
    )
    .map(({ item }) => item)
}

const byName = <T extends { id: string; name: string }>(items: T[]) =>
  [...items].sort(
    (a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()) || a.id.localeCompare(b.id),
  )

/**
 * Each repository URL more than one system declares, as the native process derives it at each
 * read: by exact equality, in the order first declared (applications, then resources, in the
 * snapshot's order), with every system that declares it.
 */
export function sharedRepositories(
  applications: InventoryApplication[],
  resources: InventoryResource[],
): SharedRepository[] {
  const found = new Map<string, EntityRef[]>()
  const systems: { subject: EntityRef; system: InventoryApplication | InventoryResource }[] = [
    ...applications.map((system) => ({
      subject: { type: 'application' as const, id: system.id },
      system,
    })),
    ...resources.map((system) => ({
      subject: { type: 'resource' as const, id: system.id },
      system,
    })),
  ]
  for (const { subject, system } of systems)
    for (const reference of system.references) {
      if (reference.kind !== 'repository') continue
      const declaring = found.get(reference.target) ?? []
      if (!declaring.some((item) => sameSubject(item, subject))) declaring.push(subject)
      found.set(reference.target, declaring)
    }
  return [...found]
    .filter(([, declaring]) => declaring.length > 1)
    .map(([target, declaring]) => ({ target, systems: declaring }))
}

/**
 * Where a remote or a repository URL points, as the native process compares them (see
 * `remotes::location`): host and path, regardless of protocol, user, port, case, surrounding
 * slashes and a trailing `.git`. Null for what is not compared: a path on disk, a remote helper.
 */
export function remoteLocation(url: string): string | null {
  const text = url.trim()
  let host: string
  let path: string
  if (/^(https?|ssh|git):\/\//i.test(text)) {
    // Read the authority by hand: the URL parser keeps unknown schemes' hosts opaque.
    const match = /^[a-z]+:\/\/(?:[^@/]*@)?([^/:]+)(?::\d+)?(\/.*)?$/i.exec(text)
    if (!match) return null
    host = match[1]!
    path = match[2] ?? ''
  } else {
    const scp = /^(?:[^@/:]+@)?([^/:]+):(.*)$/.exec(text)
    if (!scp || text.includes('::') || /^[a-z]:[\\/]/i.test(text)) return null
    host = scp[1]!
    path = scp[2]!
  }
  path = path
    .replace(/^\/+|\/+$/g, '')
    .replace(/\.git$/, '')
    .replace(/\/+$/, '')
  if (!host || !path) return null
  return `${host.toLowerCase()}/${path.toLowerCase()}`
}

/** Whether a reading found work that existed only in its copy. */
function holdsLocalWork(reading: CopyReading) {
  const some = (count: number | null) => (count ?? 0) > 0
  return (
    reading.outcome === 'read' &&
    (some(reading.unpublishedOnHead) ||
      some(reading.unpublishedElsewhere) ||
      some(reading.stashes) ||
      some(reading.changed) ||
      some(reading.untracked) ||
      some(reading.unverifiable) ||
      reading.operation !== null)
  )
}

/**
 * Each local copy the systems declare, with its newest reading and what the native process
 * derives from it, in the snapshot's order: applications, then resources, each in the order of its
 * references.
 */
export function localCopies(
  applications: InventoryApplication[],
  resources: InventoryResource[],
  newestReadings: CopyReading[],
): LocalCopy[] {
  const systems = [
    ...applications.map((system) => ({
      subject: { type: 'application' as const, id: system.id },
      system,
    })),
    ...resources.map((system) => ({
      subject: { type: 'resource' as const, id: system.id },
      system,
    })),
  ]
  return systems.flatMap(({ subject, system }) => {
    const declared = system.references
      .filter((reference) => reference.kind === 'repository')
      .map((reference) => remoteLocation(reference.target))
      .filter((location) => location !== null)
    return system.references
      .filter((reference) => reference.kind === 'local_copy' && reference.machineId)
      .map((reference) => {
        const latest =
          newestReadings.find((reading) => reading.referenceId === reference.id) ?? null
        const found = (latest?.remotes ?? [])
          .map((remote) => (remote.url ? remoteLocation(remote.url) : null))
          .filter((location) => location !== null)
        const repository = !found.length
          ? null
          : !declared.length
            ? ('undeclared' as const)
            : found.some((location) => declared.includes(location))
              ? ('matches' as const)
              : ('differs' as const)
        return {
          referenceId: reference.id,
          subject,
          machineId: reference.machineId!,
          path: reference.target,
          latest,
          movedSinceRead:
            latest !== null &&
            (latest.path !== reference.target || latest.machineId !== reference.machineId),
          repository,
          localWork: latest !== null && holdsLocalWork(latest),
        }
      })
  })
}

/**
 * The read commands of the native process over sample data, with its windowing and paging
 * rules: every manual record but only the latest automated results of each system, and older
 * ones a page at a time from an anchor. Every write is refused. Only the preview loads this.
 */
export function previewBackend(database: PreviewDatabase) {
  const observations = newestFirst(
    database.observations,
    (item) => item.observedAt,
    (item) => item.recordedAt,
  )
  const events = newestFirst(
    database.events,
    (item) => item.occurredAt,
    (item) => item.recordedAt,
  )
  const readings = newestFirst(
    database.copyReadings,
    (item) => item.observedAt,
    (item) => item.recordedAt,
  )
  const exists = (subject: EntityRef) =>
    subjectsOf(database).some((item) => sameSubject(item, subject))
  const automated = (item: Observation | RecordedEvent) => item.source === 'http'
  const unavailable = () => {
    if (database.unavailable)
      throw {
        code: 'database_error',
        message: 'Inventory database error: unable to open database file',
      }
  }

  function list(): InventorySnapshot {
    unavailable()
    const kept = new Set<string>()
    const truncated: InventorySnapshot['automatedHistory']['truncated'] = []
    for (const subject of subjectsOf(database)) {
      const own = observations.filter(
        (item) => automated(item) && sameSubject(item.subject, subject),
      )
      for (const item of own.slice(0, limit)) kept.add(item.id)
      if (own.length > limit) truncated.push({ subject, since: own[limit - 1]!.observedAt })
    }
    const applications = byName(database.applications)
    const resources = byName(database.resources)
    return {
      applications,
      resources,
      sharedRepositories: sharedRepositories(applications, resources),
      localCopies: localCopies(applications, resources, readings),
      observations: observations.filter((item) => !automated(item) || kept.has(item.id)),
      incidents: [...database.incidents].sort(
        (a, b) => Date.parse(b.openedAt) - Date.parse(a.openedAt),
      ),
      events: events.filter(
        (item) => !automated(item) || (item.observationId !== null && kept.has(item.observationId)),
      ),
      httpChecks: database.httpChecks,
      changes: database.changes,
      changesSince: database.changesSince,
      automatedHistory: { limit, truncated },
    }
  }

  function olderObservations(subject: EntityRef, before: string): ObservationPage {
    if (!exists(subject)) throw invalid(`The ${subject.type} ${subject.id} does not exist.`)
    const anchor = observations.findIndex(
      (item) => item.id === before && sameSubject(item.subject, subject),
    )
    if (anchor === -1) throw invalid(`Observation ${before} is not one of this ${subject.type}'s.`)
    const older = observations
      .slice(anchor + 1)
      .filter((item) => automated(item) && sameSubject(item.subject, subject))
    return { observations: older.slice(0, limit), more: older.length > limit }
  }

  function olderEvents(subjects: EntityRef[], before: string): EventPage {
    if (!subjects.length) throw invalid('Choose at least one application or resource.')
    const missing = subjects.find((subject) => !exists(subject))
    if (missing) throw invalid(`The ${missing.type} ${missing.id} does not exist.`)
    const ours = (subject: EntityRef) => subjects.some((item) => sameSubject(item, subject))
    const anchor = events.findIndex((item) => item.id === before && ours(item.subject))
    if (anchor === -1) throw invalid(`Event ${before} is not one of these systems' events.`)
    const older = events.slice(anchor + 1).filter((item) => automated(item) && ours(item.subject))
    return { events: older.slice(0, limit), more: older.length > limit }
  }

  function olderReadings(subject: EntityRef, before: string | null): ReadingPage {
    if (!exists(subject)) throw invalid(`The ${subject.type} ${subject.id} does not exist.`)
    const own = readings.filter((item) => sameSubject(item.subject, subject))
    const anchor = before === null ? -1 : own.findIndex((item) => item.id === before)
    if (before !== null && anchor === -1)
      throw invalid(`Reading ${before} is not one of this ${subject.type}'s.`)
    const older = own.slice(anchor + 1)
    return { readings: older.slice(0, limit), more: older.length > limit }
  }

  /** What supervision would report: each active check due one interval after its last run. */
  function monitorStatus(): MonitorStatus {
    return {
      checks: database.httpChecks.map((check) => {
        const last = observations.find(
          (item) => automated(item) && sameSubject(item.subject, check.subject),
        )
        const next =
          last && check.monitor === 'active' && check.monitorIntervalMinutes
            ? new Date(
                Date.parse(last.observedAt) + check.monitorIntervalMinutes * 60_000,
              ).toISOString()
            : null
        return {
          subject: check.subject,
          running: false,
          lastRunAt: last?.observedAt ?? null,
          nextRunAt: next,
          lastError: null,
        }
      }),
      error: null,
      recorded: 0,
      changes: 0,
    }
  }

  return (command: string, args: Record<string, unknown> = {}): unknown => {
    switch (command) {
      case 'list_inventory':
        return list()
      case 'data_location':
        return {
          database: `${folder}\\inventory.sqlite3`,
          directory: folder,
          storedAt: null,
        } satisfies DataLocation
      // Listing reads the folder, not the inventory. Backing up writes, so it is refused.
      case 'list_backups':
        return { directory: `${folder}\\backups`, files: database.backups } satisfies BackupFolder
      // The preview's sample is closed to agents; opening it writes, so it is refused.
      case 'agent_access':
        return {
          mode: 'off',
          changedAt: null,
          executable: 'C:\\Users\\Juan Pablo\\AppData\\Local\\DevHQ\\devhq.exe',
        } satisfies AgentAccess
      case 'older_observations':
        return olderObservations(args.subject as EntityRef, args.before as string)
      case 'older_events':
        return olderEvents(args.subjects as EntityRef[], args.before as string)
      case 'watch_http_monitor':
        ;(args.channel as Channel<MonitorStatus>).onmessage(monitorStatus())
        return null
      // Choosing a workstation and reading copies write, so they are refused.
      case 'this_workstation':
        unavailable()
        return database.thisWorkstation satisfies ThisWorkstation
      case 'older_readings':
        return olderReadings(args.subject as EntityRef, (args.before as string | null) ?? null)
      default:
        throw readOnly
    }
  }
}
