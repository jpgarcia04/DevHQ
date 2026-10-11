import { describe, expect, it, vi } from 'vitest'
import { sameSubject } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import type {
  BackupFolder,
  EventPage,
  InventorySnapshot,
  MonitorStatus,
  ObservationPage,
  ReadingPage,
  ThisWorkstation,
} from '@/features/inventory/inventory-api'
import { previewBackend, readOnly, remoteLocation, sharedRepositories } from './backend'
import { dotfilesReadings, sampleDatabase } from './sample'

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
      'Home desktop',
      'Web host',
    ])
  })

  it('nests each system’s references and derives the repositories several systems share', () => {
    const { database, snapshot } = preview()
    // As stored, in the order they were saved, under their own system.
    expect(snapshot.applications.find((item) => item.id === 'shop')!.references).toEqual(
      database.applications.find((item) => item.id === 'shop')!.references,
    )
    expect(snapshot.resources.find((item) => item.id === 'web-host')!.references).toEqual([
      {
        id: 'web-logs',
        kind: 'logs',
        target: '/var/log/nginx',
        label: 'Nginx',
        machineId: 'web-host',
      },
    ])
    // Blog and Shop are parts of one monorepo: the snapshot's order, applications first.
    expect(snapshot.sharedRepositories).toEqual([
      {
        target: 'https://github.com/example/web',
        systems: [
          { type: 'application', id: 'blog' },
          { type: 'application', id: 'shop' },
        ],
      },
    ])
  })

  it('compares repositories as the native process does: by exact URL, among systems', () => {
    const system = (id: string, ...targets: [string, 'repository' | 'documentation'][]) => ({
      id,
      name: id,
      description: '',
      lifecycle: 'production' as const,
      version: null,
      components: [],
      references: targets.map(([target, kind], index) => ({
        id: `${id}-${index}`,
        kind,
        target,
        label: null,
        machineId: null,
      })),
    })
    const repo = 'https://github.com/me/platform'
    const shared = sharedRepositories(
      [
        system('api', [repo, 'repository']),
        // Another spelling of the same place is another repository; a document is no repository.
        system('docs', [`${repo}/`, 'repository'], [repo, 'documentation']),
        system('web', [repo, 'repository'], [repo, 'repository']),
        system('blog', ['https://github.com/me/blog', 'repository']),
      ],
      [
        {
          id: 'host',
          name: 'Host',
          kind: 'server',
          description: '',
          operatingSystem: '',
          references: [{ id: 'h', kind: 'repository', target: repo, label: null, machineId: null }],
        },
      ],
    )
    expect(shared).toEqual([
      {
        target: repo,
        systems: [
          { type: 'application', id: 'api' },
          { type: 'application', id: 'web' },
          { type: 'resource', id: 'host' },
        ],
      },
    ])
  })

  it('carries each declared local copy with its newest reading, as the native process derives it', () => {
    const { backend, snapshot } = preview()
    expect(
      snapshot.localCopies.map((copy) => [
        copy.referenceId,
        copy.latest?.id ?? null,
        copy.repository,
        copy.localWork,
        copy.movedSinceRead,
      ]),
    ).toEqual([
      // Applications first, in the snapshot's order: Blog's clone on the laptop was never read;
      // its clone on the desktop has a remote that is not the repository Blog declares.
      ['blog-clone', null, null, false, false],
      ['blog-desktop', 'blog-desktop-read', 'differs', true, false],
      // An archived application's copy keeps its reading and what it held.
      ['old-site-clone', 'old-site-read', 'undeclared', true, false],
      ['shop-clone', 'reading-new', 'matches', true, false],
      // The laptop's dotfiles were last read where they no longer are, and not found there.
      ['laptop-dotfiles', `dotfiles-${dotfilesReadings - 1}`, null, false, true],
    ])
    expect(backend('this_workstation')).toMatchObject({ resourceId: 'laptop', problem: null })
    // A system's readings, newest first, from the newest or from one already shown.
    const shop: EntityRef = { type: 'application', id: 'shop' }
    const page = backend('older_readings', { subject: shop, before: null }) as ReadingPage
    expect([ids(page.readings), page.more]).toEqual([
      ['reading-new', 'reading-old', 'reading-gone'],
      false,
    ])
    expect(
      ids(
        (backend('older_readings', { subject: shop, before: 'reading-new' }) as ReadingPage)
          .readings,
      ),
    ).toEqual(['reading-old', 'reading-gone'])
    // More readings than a page: 200, then the rest, without gaps or repeats.
    const laptop: EntityRef = { type: 'resource', id: 'laptop' }
    const first = backend('older_readings', { subject: laptop, before: null }) as ReadingPage
    expect([first.readings.length, first.more]).toEqual([200, true])
    const rest = backend('older_readings', {
      subject: laptop,
      before: first.readings.at(-1)!.id,
    }) as ReadingPage
    expect([rest.readings.length, rest.more]).toEqual([dotfilesReadings - 200, false])
    expect(new Set(ids([...first.readings, ...rest.readings])).size).toBe(dotfilesReadings)
    expect(() => backend('older_readings', { subject: web, before: 'reading-new' })).toThrow()
    // Reading a copy and choosing this computer write: refused.
    for (const command of ['read_copies', 'set_this_workstation'])
      expect(() => backend(command, {})).toThrow(expect.objectContaining(readOnly))
    expect(
      (previewBackend(sampleDatabase('empty', now))('this_workstation') as ThisWorkstation).problem,
    ).toBe('not_chosen')
  })

  it('compares a remote with a declared repository wherever both point', () => {
    for (const same of [
      'git@github.com:me/shop.git',
      'ssh://git@GitHub.com:22/Me/Shop',
      'https://token@github.com/me/shop/',
    ])
      expect(remoteLocation(same)).toBe(remoteLocation('https://github.com/me/shop'))
    for (const nothing of [
      '/srv/git/shop.git',
      'C:\\git\\shop',
      'ext::sh -c x',
      'https://github.com/',
    ])
      expect(remoteLocation(nothing)).toBeNull()
    expect(remoteLocation('git@gitlab.com:me/shop.git')).not.toBe(
      remoteLocation('https://github.com/me/shop'),
    )
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

  it('refuses to back up, like every write, but lists a backups folder', () => {
    const { backend } = preview()
    expect(() => backend('backup_inventory')).toThrow(expect.objectContaining(readOnly))
    const folder = backend('list_backups') as BackupFolder
    expect(folder.directory).toBe('C:\\Users\\you\\AppData\\Roaming\\com.devhq.desktop\\backups')
    // Named as the native process names them, newest first, with an interrupted one.
    expect(folder.files.map((file) => [file.name, file.partial])).toEqual([
      ['inventory-20260930T103000.000Z.sqlite3', false],
      ['inventory-20260927T120000.000Z.sqlite3.partial', true],
      ['inventory-20260921T120000.000Z.sqlite3', false],
    ])
    // The folder can be read even when the inventory cannot.
    expect(previewBackend(sampleDatabase('unavailable', now))('list_backups')).toEqual(folder)
    expect(previewBackend(sampleDatabase('empty', now))('list_backups')).toEqual({
      ...folder,
      files: [],
    })
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
