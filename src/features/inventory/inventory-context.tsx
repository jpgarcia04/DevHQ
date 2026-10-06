import { createContext, useContext } from 'react'
import type { EntityRef, Observation } from '@/domain/models'
import type {
  AgentAccess,
  AgentAccessMode,
  ApplicationInput,
  Backup,
  BackupFolder,
  DataLocation,
  EventInput,
  EventPage,
  HttpCheckInput,
  IncidentInput,
  IncidentStatusInput,
  InventorySnapshot,
  MonitorStatus,
  ObservationInput,
  ObservationPage,
  ResourceInput,
} from './inventory-api'

export type Mode = 'demo' | 'real'
export type LoadState = 'idle' | 'loading' | 'ready' | 'error'
export interface InventoryContextValue {
  mode: Mode
  nativeAvailable: boolean
  setMode: (mode: Mode) => void
  snapshot: InventorySnapshot | null
  loadState: LoadState
  error: string | null
  /** Clock used to judge validity. Refreshed on load and periodically while open. */
  now: number
  reload: () => Promise<boolean>
  /** Where the database file is; it reads nothing from it, so it works when loading fails. */
  dataLocation: () => Promise<DataLocation>
  /** Copies the inventory into its backups folder; it changes nothing in the inventory. */
  backupInventory: () => Promise<Backup>
  /** The files in the backups folder; it reads none of them. */
  listBackups: () => Promise<BackupFolder>
  /** Whether agents may use the agent gate; it reads nothing else. */
  agentAccess: () => Promise<AgentAccess>
  setAgentAccess: (mode: AgentAccessMode) => Promise<AgentAccess>
  saveResource: (input: ResourceInput) => Promise<void>
  saveApplication: (input: ApplicationInput) => Promise<void>
  deleteApplication: (id: string) => Promise<void>
  deleteResource: (id: string) => Promise<void>
  recordObservation: (input: ObservationInput) => Promise<void>
  /** Reads automated observations the snapshot left out; it writes nothing. */
  olderObservations: (subject: EntityRef, before: string) => Promise<ObservationPage>
  /** Reads automated events a timeline left out; it writes nothing. */
  olderEvents: (subjects: EntityRef[], before: string) => Promise<EventPage>
  openIncident: (input: IncidentInput) => Promise<void>
  updateIncidentStatus: (input: IncidentStatusInput) => Promise<void>
  recordEvent: (input: EventInput) => Promise<void>
  saveHttpCheck: (input: HttpCheckInput) => Promise<void>
  removeHttpCheck: (subject: EntityRef) => Promise<void>
  setHttpCheckPaused: (subject: EntityRef, paused: boolean) => Promise<void>
  /** Runs the subject's HTTP check once; resolves with the observation it recorded. */
  runHttpCheck: (subject: EntityRef) => Promise<Observation>
  /** Subject keys whose HTTP check was asked to run from this window. */
  checking: string[]
  /** Supervision as the native process reports it; null until it first reports. */
  monitor: MonitorStatus | null
  /** The window could not receive supervision status. */
  monitorError: string | null
}

export const InventoryContext = createContext<InventoryContextValue | null>(null)

export function useInventory() {
  const value = useContext(InventoryContext)
  if (!value) throw new Error('InventoryProvider is missing')
  return value
}
