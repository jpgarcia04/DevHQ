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
  // What the last status said, to tell when the inventory may have changed since.
  const seen = useRef<{ recorded: number; changes: number } | null>(null)
  // Reads overlap: a quiet read can still be in flight when a write's reload starts, and answer
  // after it. Each read takes a ticket; an answer older than one already applied is dropped,
  // so the window never goes back to a state from before a write it has shown.
  const issued = useRef(0)
  const applied = useRef(0)

  const read = useCallback(async () => {
    const ticket = ++issued.current
    try {
      const next = await inventoryApi.list()
      if (ticket > applied.current) {
        applied.current = ticket
        setSnapshot(next)
        setNow(Date.now())
        setError(null)
        setLoadState('ready')
      }
      return true
    } catch (cause) {
      if (ticket > applied.current) {
        applied.current = ticket
        setSnapshot(null)
        setError(inventoryError(cause))
        setLoadState('error')
      }
      return false
    }
  }, [])

  const reload = useCallback(async () => {
    setLoadState('loading')
    setError(null)
    return read()
  }, [read])

  // New evidence from a scheduled run, or a change another process committed, appears without
  // a loading state, so the page, open forms and expanded records stay as they are. A failure is
  // shown like any failed load.
  const refresh = useCallback(() => void read(), [read])

  useEffect(() => {
    if (mode !== 'real') return
    Promise.resolve().then(() => void reload())
    const timer = window.setInterval(() => setNow(Date.now()), clockInterval)
    return () => window.clearInterval(timer)
  }, [mode, reload])

  useEffect(() => {
    if (mode !== 'real') return
    let current = true
    seen.current = null
    inventoryApi
      .watchHttpMonitor((status) => {
        if (!current) return
        setMonitor(status)
        setMonitorError(null)
        // Runs that recorded evidence, or commits by other connections, since the last status:
        // read the inventory again. The first status compares with launch, when both were 0.
        const before = seen.current ?? { recorded: 0, changes: 0 }
        if (status.recorded !== before.recorded || status.changes !== before.changes) refresh()
        seen.current = { recorded: status.recorded, changes: status.changes }
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
        dataLocation: inventoryApi.dataLocation,
        backupInventory: inventoryApi.backupInventory,
        listBackups: inventoryApi.listBackups,
        agentAccess: inventoryApi.agentAccess,
        setAgentAccess: inventoryApi.setAgentAccess,
        saveResource: (input) => persist(inventoryApi.saveResource(input), 'Resource'),
        saveApplication: (input) => persist(inventoryApi.saveApplication(input), 'Application'),
        deleteApplication: (id) =>
          persist(inventoryApi.deleteApplication(id), 'Application deletion'),
        deleteResource: (id) => persist(inventoryApi.deleteResource(id), 'Resource deletion'),
        recordObservation: (input) => persist(inventoryApi.recordObservation(input), 'Observation'),
        olderObservations: inventoryApi.olderObservations,
        olderEvents: inventoryApi.olderEvents,
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
