import { Channel, invoke } from '@tauri-apps/api/core'
import type {
  ComponentKind,
  EntityRef,
  EventOutcome,
  IncidentSeverity,
  IncidentStatus,
  Lifecycle,
  ManualEventKind,
  Observation,
  ObservationResult,
  RecordedEvent,
  RecordedIncident,
} from '@/domain/models'

export interface InventoryResource {
  id: string
  name: string
  kind: 'server' | 'workstation'
  description: string
  operatingSystem: string
}

export interface InventoryComponent {
  id: string
  name: string
  kind: ComponentKind
  resourceId: string | null
}

export interface InventoryApplication {
  id: string
  name: string
  description: string
  lifecycle: Lifecycle
  version: string | null
  components: InventoryComponent[]
}

/**
 * `off`: runs only when asked. `active`: also every interval while DevHQ is open.
 * `paused`: keeps its interval but runs only when asked until resumed.
 */
export type MonitorState = 'off' | 'active' | 'paused'

/** The one HTTP check an application or resource can have. */
export interface HttpCheck {
  subject: EntityRef
  url: string
  expectedStatus: number
  timeoutSeconds: number
  validForMinutes: number
  updatedAt: string
  monitor: MonitorState
  monitorIntervalMinutes: number | null
}

/** What only the running DevHQ process knows about a check's supervision. */
export interface MonitorCheckStatus {
  subject: EntityRef
  running: boolean
  lastRunAt: string | null
  /** Only while supervision is active and no run is in flight. */
  nextRunAt: string | null
  /** The last run failed inside DevHQ and recorded nothing. */
  lastError: { message: string; at: string } | null
}

export interface MonitorStatus {
  checks: MonitorCheckStatus[]
  /** DevHQ could not read the checks' configuration. */
  error: string | null
  /** Observations recorded by runs since launch; grows when there is new evidence. */
  recorded: number
}

/** One consistent read of SQLite. Records are newest first; times are UTC RFC 3339. */
export interface InventorySnapshot {
  applications: InventoryApplication[]
  resources: InventoryResource[]
  observations: Observation[]
  incidents: RecordedIncident[]
  events: RecordedEvent[]
  httpChecks: HttpCheck[]
}

export type ResourceInput = Omit<InventoryResource, 'id'> & { id?: string }
export type ApplicationInput = Omit<InventoryApplication, 'id' | 'components'> & {
  id?: string
  components: (Omit<InventoryComponent, 'id'> & { id?: string })[]
}
export interface ObservationInput {
  subject: EntityRef
  result: ObservationResult
  summary: string
  observedAt: string
  validForMinutes: number
}
export interface IncidentInput {
  subject: EntityRef
  title: string
  description: string
  severity: IncidentSeverity
}
export interface IncidentStatusInput {
  id: string
  status: IncidentStatus
  note: string
}
/**
 * `monitorIntervalMinutes: null` turns supervision off. Turning it on starts it active;
 * editing a supervised check keeps it active or paused as it was.
 */
export type HttpCheckInput = Omit<HttpCheck, 'updatedAt' | 'monitor'>
export interface EventInput {
  subject: EntityRef
  kind: ManualEventKind
  outcome: EventOutcome
  title: string
  detail: string
  occurredAt: string
}

export const inventoryApi = {
  list: () => invoke<InventorySnapshot>('list_inventory'),
  saveResource: (input: ResourceInput) => invoke<InventoryResource>('save_resource', { input }),
  saveApplication: (input: ApplicationInput) =>
    invoke<InventoryApplication>('save_application', { input }),
  recordObservation: (input: ObservationInput) =>
    invoke<Observation>('record_observation', { input }),
  openIncident: (input: IncidentInput) => invoke<RecordedIncident>('open_incident', { input }),
  updateIncidentStatus: (input: IncidentStatusInput) =>
    invoke<RecordedIncident>('update_incident_status', { input }),
  recordEvent: (input: EventInput) => invoke<RecordedEvent>('record_event', { input }),
  saveHttpCheck: (input: HttpCheckInput) => invoke<HttpCheck>('save_http_check', { input }),
  removeHttpCheck: (subject: EntityRef) => invoke<null>('remove_http_check', { subject }),
  setHttpCheckPaused: (subject: EntityRef, paused: boolean) =>
    invoke<HttpCheck>('set_http_check_paused', { subject, paused }),
  /**
   * Resolves with the recorded observation, including when the checked system failed. If a
   * run is already in flight, resolves with that run's observation instead of starting another.
   */
  runHttpCheck: (subject: EntityRef) => invoke<Observation>('run_http_check', { subject }),
  /** Receives the supervision status now and after every change, pushed by the native process. */
  watchHttpMonitor: async (onStatus: (status: MonitorStatus) => void) => {
    const channel = new Channel<MonitorStatus>(onStatus)
    await invoke<null>('watch_http_monitor', { channel })
  },
}

export function inventoryError(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message: unknown }).message
    if (typeof message === 'string') return message
  }
  if (typeof error === 'string') return error
  return 'The inventory request failed. Check the local database and try again.'
}
