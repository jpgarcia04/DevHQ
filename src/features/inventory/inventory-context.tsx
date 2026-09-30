import { createContext, useContext } from 'react'
import type {
  ApplicationInput,
  EventInput,
  IncidentInput,
  IncidentStatusInput,
  InventorySnapshot,
  ObservationInput,
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
  saveResource: (input: ResourceInput) => Promise<void>
  saveApplication: (input: ApplicationInput) => Promise<void>
  recordObservation: (input: ObservationInput) => Promise<void>
  openIncident: (input: IncidentInput) => Promise<void>
  updateIncidentStatus: (input: IncidentStatusInput) => Promise<void>
  recordEvent: (input: EventInput) => Promise<void>
}

export const InventoryContext = createContext<InventoryContextValue | null>(null)

export function useInventory() {
  const value = useContext(InventoryContext)
  if (!value) throw new Error('InventoryProvider is missing')
  return value
}
