import { useCallback, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { inventoryApi, inventoryError, type InventorySnapshot } from './inventory-api'
import { InventoryContext, type LoadState, type Mode } from './inventory-context'

const nativeAvailable = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
// Re-evaluates validity while the window stays open. It reads no data and runs no checks.
const clockInterval = 30_000

export function InventoryProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<Mode>(nativeAvailable ? 'real' : 'demo')
  const [snapshot, setSnapshot] = useState<InventorySnapshot | null>(null)
  const [loadState, setLoadState] = useState<LoadState>('idle')
  const [error, setError] = useState<string | null>(null)
  const [now, setNow] = useState(() => Date.now())

  const reload = useCallback(async () => {
    setLoadState('loading')
    setError(null)
    try {
      setSnapshot(await inventoryApi.list())
      setNow(Date.now())
      setLoadState('ready')
      return true
    } catch (cause) {
      setSnapshot(null)
      setError(inventoryError(cause))
      setLoadState('error')
      return false
    }
  }, [])

  useEffect(() => {
    if (mode !== 'real') return
    Promise.resolve().then(() => void reload())
    const timer = window.setInterval(() => setNow(Date.now()), clockInterval)
    return () => window.clearInterval(timer)
  }, [mode, reload])

  // A write is only reported as done once the reloaded snapshot shows it.
  const persist = async (write: Promise<unknown>, record: string) => {
    await write
    if (!(await reload()))
      throw new Error(`${record} saved, but the inventory could not be reloaded. Retry loading it.`)
  }

  return (
    <InventoryContext.Provider
      value={{
        mode,
        nativeAvailable,
        setMode,
        snapshot,
        loadState,
        error,
        now,
        reload,
        saveResource: (input) => persist(inventoryApi.saveResource(input), 'Resource'),
        saveApplication: (input) => persist(inventoryApi.saveApplication(input), 'Application'),
        recordObservation: (input) => persist(inventoryApi.recordObservation(input), 'Observation'),
        openIncident: (input) => persist(inventoryApi.openIncident(input), 'Incident'),
        updateIncidentStatus: (input) =>
          persist(inventoryApi.updateIncidentStatus(input), 'Incident change'),
        recordEvent: (input) => persist(inventoryApi.recordEvent(input), 'Event'),
      }}
    >
      {children}
    </InventoryContext.Provider>
  )
}
