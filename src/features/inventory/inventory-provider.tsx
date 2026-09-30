import { useCallback, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import {
  inventoryApi,
  inventoryError,
  type ApplicationInput,
  type InventorySnapshot,
  type ResourceInput,
} from './inventory-api'
import { InventoryContext, type LoadState, type Mode } from './inventory-context'

const nativeAvailable = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export function InventoryProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<Mode>(nativeAvailable ? 'real' : 'demo')
  const [snapshot, setSnapshot] = useState<InventorySnapshot | null>(null)
  const [loadState, setLoadState] = useState<LoadState>('idle')
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(async () => {
    setLoadState('loading')
    setError(null)
    try {
      setSnapshot(await inventoryApi.list())
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
    if (mode === 'real') Promise.resolve().then(() => void reload())
  }, [mode, reload])

  const saveResource = async (input: ResourceInput) => {
    await inventoryApi.saveResource(input)
    if (!(await reload()))
      throw new Error('Resource saved, but the inventory could not be reloaded. Retry loading it.')
  }
  const saveApplication = async (input: ApplicationInput) => {
    await inventoryApi.saveApplication(input)
    if (!(await reload()))
      throw new Error(
        'Application saved, but the inventory could not be reloaded. Retry loading it.',
      )
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
        reload,
        saveResource,
        saveApplication,
      }}
    >
      {children}
    </InventoryContext.Provider>
  )
}
