import { createContext, useContext } from 'react'
import { type ApplicationInput, type InventorySnapshot, type ResourceInput } from './inventory-api'

export type Mode = 'demo' | 'real'
export type LoadState = 'idle' | 'loading' | 'ready' | 'error'
export interface InventoryContextValue {
  mode: Mode
  nativeAvailable: boolean
  setMode: (mode: Mode) => void
  snapshot: InventorySnapshot | null
  loadState: LoadState
  error: string | null
  reload: () => Promise<boolean>
  saveResource: (input: ResourceInput) => Promise<void>
  saveApplication: (input: ApplicationInput) => Promise<void>
}

export const InventoryContext = createContext<InventoryContextValue | null>(null)

export function useInventory() {
  const value = useContext(InventoryContext)
  if (!value) throw new Error('InventoryProvider is missing')
  return value
}
