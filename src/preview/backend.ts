import type { Channel } from '@tauri-apps/api/core'
import { sameSubject, subjectsOf } from '@/domain/evidence'
import type { EntityRef, Observation, RecordedEvent } from '@/domain/models'
import type {
  AgentAccess,
  BackupFolder,
  DataLocation,
  EventPage,
  InventorySnapshot,
  MonitorStatus,
  ObservationPage,
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
  const exists = (subject: EntityRef) =>
    subjectsOf(database).some((item) => sameSubject(item, subject))
  const automated = (item: Observation | RecordedEvent) => item.source === 'http'

  function list(): InventorySnapshot {
    if (database.unavailable)
      throw {
        code: 'database_error',
        message: 'Inventory database error: unable to open database file',
      }
    const kept = new Set<string>()
    const truncated: InventorySnapshot['automatedHistory']['truncated'] = []
    for (const subject of subjectsOf(database)) {
      const own = observations.filter(
        (item) => automated(item) && sameSubject(item.subject, subject),
      )
      for (const item of own.slice(0, limit)) kept.add(item.id)
      if (own.length > limit) truncated.push({ subject, since: own[limit - 1]!.observedAt })
    }
    return {
      applications: byName(database.applications),
      resources: byName(database.resources),
      observations: observations.filter((item) => !automated(item) || kept.has(item.id)),
      incidents: [...database.incidents].sort(
        (a, b) => Date.parse(b.openedAt) - Date.parse(a.openedAt),
      ),
      events: events.filter(
        (item) => !automated(item) || (item.observationId !== null && kept.has(item.observationId)),
      ),
      httpChecks: database.httpChecks,
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
      default:
        throw readOnly
    }
  }
}
