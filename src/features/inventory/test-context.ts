import { vi } from 'vitest'
import type { InventoryContextValue } from './inventory-context'

// Tests judge validity against a fixed clock instead of the machine's.
export const testNow = Date.parse('2026-09-30T12:00:00Z')
export const minutesFromNow = (minutes: number) =>
  new Date(testNow + minutes * 60_000).toISOString()

export function inventoryContext(
  overrides: Partial<InventoryContextValue> = {},
): InventoryContextValue {
  return {
    mode: 'real',
    nativeAvailable: true,
    setMode: vi.fn(),
    snapshot: {
      applications: [],
      resources: [
        {
          id: 'host-1',
          name: 'Host One',
          kind: 'server',
          description: '',
          operatingSystem: 'Linux',
        },
      ],
      observations: [],
      incidents: [],
      events: [],
      httpChecks: [],
      automatedHistory: { limit: 200, truncated: [] },
    },
    loadState: 'ready',
    error: null,
    now: testNow,
    reload: vi.fn(async () => true),
    dataLocation: vi.fn(async () => ({
      database: '/home/you/.local/share/com.devhq.desktop/inventory.sqlite3',
      directory: '/home/you/.local/share/com.devhq.desktop',
    })),
    backupInventory: vi.fn(async () => {
      throw new Error('backupInventory is not mocked in this test')
    }),
    listBackups: vi.fn(async () => ({
      directory: '/home/you/.local/share/com.devhq.desktop/backups',
      files: [],
    })),
    saveResource: vi.fn(async () => {}),
    saveApplication: vi.fn(async () => {}),
    deleteApplication: vi.fn(async () => {}),
    deleteResource: vi.fn(async () => {}),
    recordObservation: vi.fn(async () => {}),
    olderObservations: vi.fn(async () => ({ observations: [], more: false })),
    olderEvents: vi.fn(async () => ({ events: [], more: false })),
    openIncident: vi.fn(async () => {}),
    updateIncidentStatus: vi.fn(async () => {}),
    recordEvent: vi.fn(async () => {}),
    saveHttpCheck: vi.fn(async () => {}),
    removeHttpCheck: vi.fn(async () => {}),
    setHttpCheckPaused: vi.fn(async () => {}),
    runHttpCheck: vi.fn(async () => {
      throw new Error('runHttpCheck is not mocked in this test')
    }),
    checking: [],
    monitor: { checks: [], error: null, recorded: 0 },
    monitorError: null,
    ...overrides,
  }
}
