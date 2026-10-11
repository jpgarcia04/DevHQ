import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import type { ReactNode } from 'react'
import {
  inventoryApi,
  type CopyReading,
  type InventorySnapshot,
  type LocalCopy,
  type ThisWorkstation,
} from './inventory-api'
import { InventoryContext, useInventory, type InventoryContextValue } from './inventory-context'
import { InventoryProvider } from './inventory-provider'
import { LocalCopiesOverview } from './local-copies-overview'
import { activityText, lastActivity, localCopiesOverview, readAllTargets } from './local-copies'
import { RealApplicationDetailPage } from './real-application-detail-page'
import { RealApplicationsPage } from './real-applications-page'
import { RealSettingsPage } from './real-settings-page'
import { inventoryContext, minutesFromNow, testNow } from './test-context'
import { Route, Routes } from 'react-router-dom'

const shop = { type: 'application' as const, id: 'shop' }
const old = { type: 'application' as const, id: 'old' }

const laptop: ThisWorkstation = {
  resourceId: 'laptop',
  chosenAt: minutesFromNow(-60 * 24),
  problem: null,
  message: null,
  file: 'C:\\Users\\you\\AppData\\Local\\com.devhq.desktop\\this-workstation.json',
  storedAt: null,
}
const notChosen: ThisWorkstation = {
  ...laptop,
  resourceId: null,
  chosenAt: null,
  problem: 'not_chosen',
  message:
    'No workstation has been chosen as this computer yet: choose one in Settings › This computer.',
}

function reading(id: string, facts: Partial<CopyReading> = {}): CopyReading {
  return {
    id,
    subject: shop,
    referenceId: 'shop-laptop',
    machineId: 'laptop',
    path: 'C:\\src\\shop',
    source: 'read',
    reader: 'libgit2 1.9.7',
    agent: null,
    observedAt: minutesFromNow(-120),
    recordedAt: minutesFromNow(-120),
    outcome: 'read',
    detail: null,
    branch: 'main',
    head: '0123456789abcdef0123456789abcdef01234567',
    lastCommitAt: minutesFromNow(-60 * 24 * 3),
    unpublishedOnHead: 0,
    unpublishedElsewhere: 0,
    stashes: 0,
    syncedAt: minutesFromNow(-60 * 24),
    remotes: [{ name: 'origin', url: 'git@github.com:me/shop.git' }],
    operation: null,
    changed: 0,
    untracked: 0,
    unverifiable: 0,
    worktreeUnknown: null,
    ...facts,
  }
}

function copy(referenceId: string, facts: Partial<LocalCopy>): LocalCopy {
  return {
    referenceId,
    subject: shop,
    machineId: 'laptop',
    path: 'C:\\src\\shop',
    latest: null,
    movedSinceRead: false,
    repository: null,
    localWork: false,
    ...facts,
  }
}

// Shop: a copy on the laptop with work only there, one on the desktop never read. Old (archived):
// a copy on the laptop with a stash, and one never read.
const working = reading('r-work', {
  head: 'fedcba9876543210fedcba9876543210fedcba98',
  unpublishedOnHead: 1001,
  stashes: 2,
  changed: 3,
  remotes: [
    { name: 'origin', url: 'git@github.com:me/shop.git' },
    { name: 'helper', url: null },
  ],
})
const copies: LocalCopy[] = [
  copy('shop-laptop', { latest: working, localWork: true, repository: 'matches' }),
  copy('shop-desktop', { machineId: 'desktop', path: '/home/me/shop' }),
  copy('old-laptop', {
    subject: old,
    path: 'C:\\src\\old',
    latest: reading('r-old', { subject: old, referenceId: 'old-laptop', stashes: 1 }),
    localWork: true,
  }),
  copy('old-unread', { subject: old, path: 'C:\\src\\old2' }),
]

const snapshot: InventorySnapshot = {
  applications: [
    {
      id: 'shop',
      name: 'Shop',
      description: '',
      lifecycle: 'production',
      version: null,
      components: [],
      references: [
        {
          id: 'shop-repo',
          kind: 'repository',
          target: 'https://github.com/me/shop',
          label: null,
          machineId: null,
        },
        {
          id: 'shop-laptop',
          kind: 'local_copy',
          target: 'C:\\src\\shop',
          label: null,
          machineId: 'laptop',
        },
        {
          id: 'shop-desktop',
          kind: 'local_copy',
          target: '/home/me/shop',
          label: null,
          machineId: 'desktop',
        },
      ],
    },
    {
      id: 'old',
      name: 'Old',
      description: '',
      lifecycle: 'archived',
      version: null,
      components: [],
      references: [
        {
          id: 'old-laptop',
          kind: 'local_copy',
          target: 'C:\\src\\old',
          label: null,
          machineId: 'laptop',
        },
        {
          id: 'old-unread',
          kind: 'local_copy',
          target: 'C:\\src\\old2',
          label: null,
          machineId: 'laptop',
        },
      ],
    },
  ],
  resources: [
    {
      id: 'laptop',
      name: 'Laptop',
      kind: 'workstation',
      description: '',
      operatingSystem: '',
      references: [],
    },
    {
      id: 'desktop',
      name: 'Desktop',
      kind: 'workstation',
      description: '',
      operatingSystem: '',
      references: [],
    },
    {
      id: 'vps',
      name: 'VPS',
      kind: 'server',
      description: '',
      operatingSystem: '',
      references: [],
    },
  ],
  sharedRepositories: [],
  localCopies: copies,
  observations: [],
  incidents: [],
  events: [],
  httpChecks: [],
  changes: [],
  changesSince: null,
  automatedHistory: { limit: 200, truncated: [] },
}

function renderWith(value: InventoryContextValue, children: ReactNode, path = '/') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <InventoryContext.Provider value={value}>{children}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

function shopDetail(value: InventoryContextValue) {
  return renderWith(
    value,
    <Routes>
      <Route path="/applications/:applicationId" element={<RealApplicationDetailPage />} />
    </Routes>,
    '/applications/shop',
  )
}

describe('what the window derives from local copies', () => {
  it('reads all of this workstation but archived applications, and nothing while unknown', () => {
    expect(readAllTargets(snapshot, laptop).map((item) => item.referenceId)).toEqual([
      'shop-laptop',
    ])
    expect(readAllTargets(snapshot, notChosen)).toEqual([])
    expect(readAllTargets(snapshot, null)).toEqual([])
  })

  it('lists for Overview what the agent gate lists, without archived applications', () => {
    const here = localCopiesOverview(snapshot, laptop)
    expect(here.work.map((item) => item.referenceId)).toEqual(['shop-laptop'])
    expect(here.neverReadHere).toEqual([])
    expect(here.neverReadElsewhere.map((item) => item.referenceId)).toEqual(['shop-desktop'])
    // Unknown, every copy never read is one this computer cannot speak for.
    expect(localCopiesOverview(snapshot, notChosen).neverReadElsewhere).toHaveLength(1)
  })

  it('gives the newest commit of the latest readings as a lower bound, never inactivity', () => {
    const name = (id: string) => (id === 'laptop' ? 'Laptop' : 'Desktop')
    const activity = lastActivity(snapshot, shop)
    expect(activity).toMatchObject({ copies: 2, neverRead: 1, basis: { id: 'r-work' } })
    expect(activityText(activity, name, testNow)).toBe(
      'Last commit 3 days ago, read on Laptop 2 hours ago · 1 copy never read',
    )
    // The newest commit wins, whichever copy holds it.
    const newer = reading('r-newer', {
      referenceId: 'shop-desktop',
      machineId: 'desktop',
      lastCommitAt: minutesFromNow(-30),
      observedAt: minutesFromNow(-10),
    })
    const both = {
      ...snapshot,
      localCopies: copies.map((item) =>
        item.referenceId === 'shop-desktop' ? { ...item, latest: newer } : item,
      ),
    }
    expect(activityText(lastActivity(both, shop), name, testNow)).toBe(
      'Last commit 30 min ago, read on Desktop 10 min ago',
    )
    // A commit dated after the reading says its date, not "Scheduled".
    const future = {
      ...both,
      localCopies: [
        copy('x', { latest: { ...newer, lastCommitAt: minutesFromNow(60 * 24 * 40) } }),
      ],
    }
    expect(activityText(lastActivity(future, shop), name, testNow)).toMatch(/^Last commit dated /)
    const unread = { ...snapshot, localCopies: [copy('a', {}), copy('b', {})] }
    expect(activityText(lastActivity(unread, shop), name, testNow)).toBe(
      'Activity unknown: none of its 2 copies was read',
    )
    const gone = {
      ...snapshot,
      localCopies: [copy('a', { latest: reading('r', { ...missing }) }), copy('b', {})],
    }
    expect(activityText(lastActivity(gone, shop), name, testNow)).toBe(
      'No commit found in the latest readings · 1 copy never read',
    )
    expect(activityText(lastActivity({ ...snapshot, localCopies: [] }, shop), name, testNow)).toBe(
      'No local copy declared',
    )
  })
})

const missing: Partial<CopyReading> = {
  outcome: 'missing',
  detail: 'Nothing is at this path.',
  branch: null,
  head: null,
  lastCommitAt: null,
  unpublishedOnHead: null,
  unpublishedElsewhere: null,
  stashes: null,
  syncedAt: null,
  remotes: null,
  changed: null,
  untracked: null,
  unverifiable: null,
}

describe('a local copy under its references', () => {
  it('shows its newest reading in the past, with its moment, machine and who asked', () => {
    shopDetail(inventoryContext({ snapshot, thisWorkstation: laptop }))
    const facts = within(screen.getByLabelText('What the reading found'))
    expect(facts.getByText('1000+ commits of HEAD on no remote branch')).toBeInTheDocument()
    expect(facts.getByText('3 changed files')).toBeInTheDocument()
    expect(facts.getByText(/URL not shown/).closest('li')).toHaveTextContent(
      /^helper · URL not shown/,
    )
    expect(screen.getByText('Automated · Local read')).toBeInTheDocument()
    expect(
      screen.getByText('A remote pointed to the repository Shop declares.'),
    ).toBeInTheDocument()
    // The desktop's copy was never read, and is not read from here.
    expect(screen.getByText(/Never read: DevHQ knows nothing/)).toBeInTheDocument()
    expect(screen.getByText(/reads it; this computer is/)).toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: /^Read / })).toHaveLength(1)
    expect(screen.getByRole('button', { name: 'Read C:\\src\\shop' })).toBeEnabled()
    // The application says what its copies say of its activity.
    expect(
      screen.getByText('Last commit 3 days ago, read on Laptop 2 hours ago · 1 copy never read'),
    ).toBeInTheDocument()
  })

  it('says why nothing is read while DevHQ does not know which workstation this is', () => {
    shopDetail(inventoryContext({ snapshot, thisWorkstation: notChosen }))
    expect(screen.queryByRole('button', { name: /^Read / })).not.toBeInTheDocument()
    expect(screen.getAllByRole('link', { name: 'choose it in Settings' })).toHaveLength(2)
  })

  it('says when the reading was made elsewhere and when no remote looks like the repository', () => {
    const moved = {
      ...snapshot,
      localCopies: [
        copy('shop-laptop', {
          latest: reading('r-moved', { path: 'C:\\old\\shop', outcome: 'read' }),
          movedSinceRead: true,
          repository: 'differs',
        }),
      ],
    }
    shopDetail(inventoryContext({ snapshot: moved, thisWorkstation: laptop }))
    expect(screen.getByText(/the copy is declared elsewhere now/)).toHaveTextContent(
      'This reading was made at C:\\old\\shop on Laptop',
    )
    expect(
      screen.getByText('None of its remotes looked like the repository Shop declares.'),
    ).toBeInTheDocument()
  })

  it('shows what the system said when a copy was not found, never facts', () => {
    const gone = {
      ...snapshot,
      localCopies: [copy('shop-laptop', { latest: reading('r', missing) })],
    }
    shopDetail(inventoryContext({ snapshot: gone, thisWorkstation: laptop }))
    expect(screen.getByText('Not found')).toBeInTheDocument()
    expect(screen.getByText('Nothing is at this path.')).toBeInTheDocument()
    expect(screen.queryByLabelText('What the reading found')).not.toBeInTheDocument()
  })

  it('reads one copy when asked and says what DevHQ could not read', async () => {
    const readCopies = vi
      .fn<InventoryContextValue['readCopies']>()
      .mockResolvedValueOnce({ readings: [working], failures: [], thisWorkstation: laptop })
      .mockResolvedValueOnce({
        readings: [],
        failures: [
          {
            referenceId: 'shop-laptop',
            subject: shop,
            path: 'C:\\src\\shop',
            message: 'Reading C:\\src\\shop took longer than 30 s, so DevHQ stopped it.',
          },
        ],
        thisWorkstation: laptop,
      })
    const user = userEvent.setup()
    shopDetail(inventoryContext({ snapshot, thisWorkstation: laptop, readCopies }))
    const read = screen.getByRole('button', { name: 'Read C:\\src\\shop' })
    await user.click(read)
    expect(readCopies).toHaveBeenCalledWith(['shop-laptop'])
    expect(await screen.findByText('Read and kept. What it found is above.')).toBeInTheDocument()
    await user.click(read)
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'took longer than 30 s, so DevHQ stopped it. Nothing was kept.',
    )
  })

  it('marks a copy being read and keeps the button from asking twice', () => {
    shopDetail(
      inventoryContext({ snapshot, thisWorkstation: laptop, readingCopies: ['shop-laptop'] }),
    )
    expect(screen.getByRole('button', { name: 'Read C:\\src\\shop' })).toBeDisabled()
    expect(screen.getByText(/without running anything it names/)).toBeInTheDocument()
  })

  it('loads every reading of the system on demand, a page at a time, and says when all are shown', async () => {
    const page = Array.from({ length: 200 }, (_, index) => reading(`r-${index}`))
    const olderReadings = vi
      .fn<InventoryContextValue['olderReadings']>()
      .mockResolvedValueOnce({ readings: page, more: true })
      .mockResolvedValueOnce({
        readings: [reading('r-last', { ...missing, referenceId: 'gone', path: 'C:\\gone' })],
        more: false,
      })
    const user = userEvent.setup()
    shopDetail(inventoryContext({ snapshot, thisWorkstation: laptop, olderReadings }))
    expect(olderReadings).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Show every reading' }))
    const history = await screen.findByRole('list', { name: 'Readings of Shop, newest first' })
    expect(olderReadings).toHaveBeenCalledWith(shop, null)
    expect(within(history).getAllByRole('listitem')).toHaveLength(200)
    expect(screen.getByText(/The newest 200 readings are shown/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Show 200 older readings' }))
    expect(olderReadings).toHaveBeenLastCalledWith(shop, 'r-199')
    expect(await screen.findByText('Every reading of Shop is shown.')).toBeInTheDocument()
    expect(within(history).getAllByRole('listitem')).toHaveLength(201)
    // A reading of a path the system no longer declares says so.
    expect(within(history).getByText(/no longer declared/)).toBeInTheDocument()
  })
})

describe('the Local copies panel', () => {
  it('lists work only in a copy and copies never read, leaving out archived applications', () => {
    renderWith(inventoryContext({ snapshot, thisWorkstation: laptop }), <LocalCopiesOverview />)
    const panel = within(screen.getByRole('heading', { name: 'Local copies' }).parentElement!)
    expect(panel.getByText('Work that existed only in a copy (1)')).toBeInTheDocument()
    expect(
      panel.getByText('1000+ unpublished commits, 2 stashes, 3 changed files'),
    ).toBeInTheDocument()
    expect(panel.queryByText('C:\\src\\old')).not.toBeInTheDocument()
    expect(panel.getByText(/1 copy on other computers was never read/)).toBeInTheDocument()
    expect(panel.getByText(/Not health: none of it is in Needs attention/)).toBeInTheDocument()
  })

  it('reads all of this computer and says which copies could not be read', async () => {
    const readCopies = vi.fn<InventoryContextValue['readCopies']>().mockResolvedValue({
      readings: [],
      failures: [
        {
          referenceId: 'shop-laptop',
          subject: shop,
          path: 'C:\\src\\shop',
          message: 'libgit2 could not open this copy: unsupported extension name.',
        },
      ],
      thisWorkstation: laptop,
    })
    const user = userEvent.setup()
    renderWith(
      inventoryContext({ snapshot, thisWorkstation: laptop, readCopies }),
      <LocalCopiesOverview />,
    )
    await user.click(screen.getByRole('button', { name: 'Read all on this computer' }))
    expect(readCopies).toHaveBeenCalledWith(null)
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Shop: libgit2 could not open this copy',
    )
    expect(
      screen.getByText('Read 0 copies; 1 could not be read and kept nothing.'),
    ).toBeInTheDocument()
  })

  it('offers no Read all while DevHQ does not know which workstation this is', () => {
    renderWith(inventoryContext({ snapshot, thisWorkstation: notChosen }), <LocalCopiesOverview />)
    expect(
      screen.getByText('DevHQ does not know which workstation this computer is'),
    ).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Read all/ })).not.toBeInTheDocument()
    expect(screen.getByText('1 copy was never read.')).toBeInTheDocument()
  })

  it('says plainly when no system declares a local copy', () => {
    renderWith(
      inventoryContext({ snapshot: { ...snapshot, localCopies: [] }, thisWorkstation: laptop }),
      <LocalCopiesOverview />,
    )
    expect(screen.getByText(/No system declares a local copy/)).toBeInTheDocument()
  })
})

describe('Settings › This computer', () => {
  it('chooses among the workstations, and only those', async () => {
    const setThisWorkstation = vi
      .fn<InventoryContextValue['setThisWorkstation']>()
      .mockResolvedValue({ ...laptop, resourceId: 'desktop' })
    const user = userEvent.setup()
    renderWith(
      inventoryContext({ snapshot, thisWorkstation: laptop, setThisWorkstation }),
      <RealSettingsPage />,
    )
    const section = within(screen.getByRole('heading', { name: 'This computer' }).parentElement!)
    expect(section.getByText(/This computer is/)).toHaveTextContent('This computer is Laptop')
    const select = section.getByRole('combobox', { name: 'Workstation' })
    expect(
      within(select)
        .getAllByRole('option')
        .map((item) => item.textContent),
    ).toEqual(['None chosen', 'Laptop', 'Desktop'])
    expect(section.getByRole('button', { name: 'Save' })).toBeDisabled()
    await user.selectOptions(select, 'desktop')
    await user.click(section.getByRole('button', { name: 'Save' }))
    expect(setThisWorkstation).toHaveBeenCalledWith('desktop')
    expect(await section.findByText(/^Saved\./)).toBeInTheDocument()
    expect(section.getByText(laptop.file)).toBeInTheDocument()
  })

  it('says why it does not know, and offers no choice the system cannot keep', () => {
    const { unmount } = renderWith(
      inventoryContext({ snapshot, thisWorkstation: notChosen }),
      <RealSettingsPage />,
    )
    expect(
      screen.getByText(/No workstation has been chosen as this computer yet/),
    ).toBeInTheDocument()
    expect(screen.getByRole('combobox', { name: 'Workstation' })).toHaveValue('')
    unmount()
    renderWith(
      inventoryContext({
        snapshot,
        thisWorkstation: {
          ...notChosen,
          problem: 'no_machine_id',
          message: 'This system gives DevHQ no machine identifier.',
        },
      }),
      <RealSettingsPage />,
    )
    expect(screen.getByText(/no machine identifier/)).toBeInTheDocument()
    expect(screen.queryByRole('combobox', { name: 'Workstation' })).not.toBeInTheDocument()
  })

  it('sends to Infrastructure when no workstation is registered', () => {
    renderWith(
      inventoryContext({
        snapshot: {
          ...snapshot,
          resources: snapshot.resources.filter((item) => item.id === 'vps'),
        },
        thisWorkstation: notChosen,
      }),
      <RealSettingsPage />,
    )
    expect(screen.getByText(/No workstation is registered/)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Infrastructure' })).toBeInTheDocument()
  })
})

function SelectRealInventory() {
  const { setMode } = useInventory()
  return <button onClick={() => setMode('real')}>Select real inventory</button>
}

describe('the provider', () => {
  afterEach(() => vi.restoreAllMocks())

  it('asks for this workstation apart from the inventory, and a failure leaves it shown', async () => {
    vi.spyOn(inventoryApi, 'watchHttpMonitor').mockResolvedValue()
    const list = vi.spyOn(inventoryApi, 'list').mockResolvedValue(snapshot)
    const ask = vi
      .spyOn(inventoryApi, 'thisWorkstation')
      .mockRejectedValue({ code: 'read_error', message: 'this-workstation.json is not JSON' })
    const user = userEvent.setup()
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <RealApplicationsPage />
          <LocalCopiesOverview />
        </InventoryProvider>
      </MemoryRouter>,
    )
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    expect(await screen.findByText(/this-workstation.json is not JSON/)).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Shop' })).toBeInTheDocument()
    expect(list).toHaveBeenCalledTimes(1)
    expect(list).toHaveBeenCalledWith()
    expect(ask).toHaveBeenCalledTimes(1)
  })

  it('reads, then reloads the inventory before saying it is done', async () => {
    vi.spyOn(inventoryApi, 'watchHttpMonitor').mockResolvedValue()
    const list = vi.spyOn(inventoryApi, 'list').mockResolvedValue(snapshot)
    vi.spyOn(inventoryApi, 'thisWorkstation').mockResolvedValue(laptop)
    const readCopies = vi
      .spyOn(inventoryApi, 'readCopies')
      .mockResolvedValue({ readings: [working], failures: [], thisWorkstation: laptop })
    const user = userEvent.setup()
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <LocalCopiesOverview />
        </InventoryProvider>
      </MemoryRouter>,
    )
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    await user.click(await screen.findByRole('button', { name: 'Read all on this computer' }))
    expect(await screen.findByText('Read 1 copy.')).toBeInTheDocument()
    expect(readCopies).toHaveBeenCalledWith(null)
    expect(list).toHaveBeenCalledTimes(2)
  })

  it('says nothing was read when the native process refused before reading', async () => {
    vi.spyOn(inventoryApi, 'watchHttpMonitor').mockResolvedValue()
    vi.spyOn(inventoryApi, 'list').mockResolvedValue(snapshot)
    vi.spyOn(inventoryApi, 'thisWorkstation').mockResolvedValue(laptop)
    vi.spyOn(inventoryApi, 'readCopies').mockRejectedValue({
      code: 'invalid_input',
      message: 'DevHQ does not know which workstation this computer is, so it reads no local copy',
    })
    const user = userEvent.setup()
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <LocalCopiesOverview />
        </InventoryProvider>
      </MemoryRouter>,
    )
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    await user.click(await screen.findByRole('button', { name: 'Read all on this computer' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'so it reads no local copy. Nothing was read.',
    )
    expect(screen.getByRole('button', { name: 'Read all on this computer' })).toBeEnabled()
  })
})
