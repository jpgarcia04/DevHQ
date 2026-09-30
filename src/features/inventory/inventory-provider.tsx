import { useCallback, useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { subjectKey } from '@/domain/evidence'
import type { EntityRef, Observation } from '@/domain/models'
import {
  inventoryApi,
  inventoryError,
  type InventorySnapshot,
  type MonitorStatus,
} from './inventory-api'
import { InventoryContext, type LoadState, type Mode } from './inventory-context'

const nativeAvailable = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
// Re-evaluates validity while the window stays open. It reads no data and runs no checks:
// supervision is planned and run by the native process, which pushes its status here.
const clockInterval = 30_000

export function InventoryProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<Mode>(nativeAvailable ? 'real' : 'demo')
  const [snapshot, setSnapshot] = useState<InventorySnapshot | null>(null)
  const [loadState, setLoadState] = useState<LoadState>('idle')
  const [error, setError] = useState<string | null>(null)
  const [now, setNow] = useState(() => Date.now())
  const [checking, setChecking] = useState<string[]>([])
  const [monitor, setMonitor] = useState<MonitorStatus | null>(null)
  const [monitorError, setMonitorError] = useState<string | null>(null)
  const recorded = useRef<number | null>(null)

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

  // New evidence from a scheduled run appears without a loading state, so the page, open
  // forms and expanded records stay as they are. A failure is shown like any failed load.
  const refresh = useCallback(async () => {
    try {
      setSnapshot(await inventoryApi.list())
      setNow(Date.now())
    } catch (cause) {
      setSnapshot(null)
      setError(inventoryError(cause))
      setLoadState('error')
    }
  }, [])

  useEffect(() => {
    if (mode !== 'real') return
    Promise.resolve().then(() => void reload())
    const timer = window.setInterval(() => setNow(Date.now()), clockInterval)
    return () => window.clearInterval(timer)
  }, [mode, reload])

  useEffect(() => {
    if (mode !== 'real') return
    let current = true
    recorded.current = null
    inventoryApi
      .watchHttpMonitor((status) => {
        if (!current) return
        setMonitor(status)
        setMonitorError(null)
        // Runs that recorded evidence since the last status: read it.
        if (status.recorded !== (recorded.current ?? 0)) void refresh()
        recorded.current = status.recorded
      })
      .catch((cause) => {
        if (current) setMonitorError(inventoryError(cause))
      })
    return () => {
      current = false
    }
  }, [mode, refresh])

  // A write is only reported as done once the reloaded snapshot shows it.
  const persist = async (write: Promise<unknown>, record: string) => {
    await write
    if (!(await reload()))
      throw new Error(`${record} saved, but the inventory could not be reloaded. Retry loading it.`)
  }

  // The request runs in the native process; the window stays usable while it does.
  const runHttpCheck = async (subject: EntityRef) => {
    const key = subjectKey(subject)
    setChecking((current) => [...current, key])
    try {
      let observation: Observation
      try {
        observation = await inventoryApi.runHttpCheck(subject)
      } catch (cause) {
        // Rust stores an observation and its event in one transaction and only then
        // answers, so a rejected call means DevHQ failed and nothing was stored.
        const message = inventoryError(cause)
        const sentence = /[.!?]$/.test(message) ? message : `${message}.`
        throw new Error(`${sentence} No observation was recorded.`, { cause })
      }
      if (!(await reload()))
        throw new Error(
          'The HTTP check result was saved, but the inventory could not be reloaded. Retry loading it.',
        )
      return observation
    } finally {
      setChecking((current) => current.filter((item) => item !== key))
    }
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
        saveHttpCheck: (input) => persist(inventoryApi.saveHttpCheck(input), 'HTTP check'),
        removeHttpCheck: (subject) =>
          persist(inventoryApi.removeHttpCheck(subject), 'HTTP check removal'),
        setHttpCheckPaused: (subject, paused) =>
          persist(inventoryApi.setHttpCheckPaused(subject, paused), paused ? 'Pause' : 'Resume'),
        runHttpCheck,
        checking,
        monitor,
        monitorError,
      }}
    >
      {children}
    </InventoryContext.Provider>
  )
}
