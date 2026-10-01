import { describe, expect, it, vi } from 'vitest'
import { sameSubject } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import type {
  EventPage,
  InventorySnapshot,
  MonitorStatus,
  ObservationPage,
} from '@/features/inventory/inventory-api'
import { previewBackend, readOnly } from './backend'
import { sampleDatabase } from './sample'

const now = Date.parse('2026-09-30T12:00:00Z')
const web: EntityRef = { type: 'resource', id: 'web-host' }
const db: EntityRef = { type: 'resource', id: 'db-host' }

function preview() {
  const database = sampleDatabase('history', now)
  const backend = previewBackend(database)
  const snapshot = backend('list_inventory') as InventorySnapshot
  return { database, backend, snapshot }
}

const ids = (items: { id: string }[]) => items.map((item) => item.id)

describe('the preview’s simulated native process', () => {
  it('carries every manual record but only the latest automated results, as a snapshot does', () => {
    const { database, snapshot } = preview()
    const webRuns = snapshot.observations.filter(
      (item) => item.source === 'http' && sameSubject(item.subject, web),
    )
    expect(webRuns).toHaveLength(200)
    expect(snapshot.automatedHistory).toEqual({
      limit: 200,
      truncated: [{ subject: web, since: webRuns.at(-1)!.observedAt }],
    })
    const manual = database.observations.filter((item) => item.source === 'manual')
    expect(snapshot.observations).toEqual(expect.arrayContaining(manual))
    // Exactly the events of what is loaded, newest first.
    expect(
      snapshot.events
        .filter((item) => item.source === 'http')
        .map((item) => item.observationId)
        .sort(),
    ).toEqual(ids(snapshot.observations.filter((item) => item.source === 'http')).sort())
    const times = snapshot.events.map((item) => Date.parse(item.occurredAt))
    expect(times).toEqual([...times].sort((a, b) => b - a))
    expect(snapshot.resources.map((item) => item.name)).toEqual([
      'Database host',
      'Dev laptop',
      'Web host',
    ])
  })

  it('pages older results from an anchor, without gaps or duplicates', () => {
    const { database, backend, snapshot } = preview()
    const everyRun = database.observations
      .filter((item) => item.source === 'http' && sameSubject(item.subject, web))
      .reverse()
    const shown = snapshot.observations.filter(
      (item) => item.source === 'http' && sameSubject(item.subject, web),
    )
    const page = backend('older_observations', {
      subject: web,
      before: shown.at(-1)!.id,
    }) as ObservationPage
    expect(page.more).toBe(false)
    expect([...ids(shown), ...ids(page.observations)]).toEqual(ids(everyRun))

    // The cut: Web host's oldest loaded result.
    const cut = snapshot.events
      .filter((item) => item.source === 'http' && sameSubject(item.subject, web))
      .at(-1)!
    const events = backend('older_events', { subjects: [web, db], before: cut.id }) as EventPage
    expect(events.events.map((item) => item.observationId)).toEqual(ids(page.observations))
  })

  it('refuses anchors of other systems, and every write', () => {
    const { backend } = preview()
    expect(() => backend('older_observations', { subject: db, before: 'web-1' })).toThrow()
    expect(() => backend('older_events', { subjects: [db], before: 'evt-web-1' })).toThrow()
    expect(() => backend('older_events', { subjects: [], before: 'evt-web-1' })).toThrow()
    for (const command of ['save_resource', 'record_observation', 'run_http_check'])
      expect(() => backend(command, {})).toThrow(expect.objectContaining(readOnly))
  })

  it('reports supervision through the window’s channel', () => {
    const { backend, snapshot } = preview()
    const channel = { onmessage: vi.fn() }
    expect(backend('watch_http_monitor', { channel })).toBeNull()
    const status = channel.onmessage.mock.calls[0]![0] as MonitorStatus
    const last = snapshot.observations.find(
      (item) => item.source === 'http' && sameSubject(item.subject, web),
    )!
    expect(status.checks.find((item) => sameSubject(item.subject, web))).toEqual({
      subject: web,
      running: false,
      lastRunAt: last.observedAt,
      nextRunAt: new Date(Date.parse(last.observedAt) + 15 * 60_000).toISOString(),
      lastError: null,
    })
    // Not supervised: no next run.
    expect(status.checks.find((item) => item.subject.type === 'application')?.nextRunAt).toBeNull()
  })

  it('can stand for an empty inventory or one that cannot be read', () => {
    expect(
      (previewBackend(sampleDatabase('empty', now))('list_inventory') as InventorySnapshot)
        .applications,
    ).toEqual([])
    expect(() => previewBackend(sampleDatabase('unavailable', now))('list_inventory')).toThrow(
      expect.objectContaining({ code: 'database_error' }),
    )
  })
})
