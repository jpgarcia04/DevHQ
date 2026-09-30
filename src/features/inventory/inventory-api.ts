import { invoke } from '@tauri-apps/api/core'
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

/** One consistent read of SQLite. Records are newest first; times are UTC RFC 3339. */
export interface InventorySnapshot {
  applications: InventoryApplication[]
  resources: InventoryResource[]
  observations: Observation[]
  incidents: RecordedIncident[]
  events: RecordedEvent[]
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
}

export function inventoryError(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message: unknown }).message
    if (typeof message === 'string') return message
  }
  if (typeof error === 'string') return error
  return 'The inventory request failed. Check the local database and try again.'
}
