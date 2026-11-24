import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import type { ReactNode } from 'react'
import {
  inventoryApi,
  type CopyReading,
  type Installation,
  type InstallationReading,
  type InventorySnapshot,
  type LocalCopy,
  type ThisWorkstation,
} from './inventory-api'
import { InventoryContext, useInventory, type InventoryContextValue } from './inventory-context'
import { InventoryProvider } from './inventory-provider'
import { PendingWorkOverview } from './pending-work-overview'
import { RealOverviewPage } from './real-overview-page'
import { inventoryContext, minutesFromNow, testNow } from './test-context'

const shop = { type: 'application' as const, id: 'shop' }
const notes = { type: 'application' as const, id: 'notes' }

const laptop: ThisWorkstation = {
  resourceId: 'laptop',
  chosenAt: minutesFromNow(-60 * 24),
  problem: null,
  message: null,
  file: 'C:\\Users\\Ana Ruiz\\DevHQ\\this-workstation.json',
  storedAt: null,
}

function copyReading(facts: Partial<CopyReading> = {}): CopyReading {
  return {
    id: 'r-shop',
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
    lastCommitAt: minutesFromNow(-60 * 24),
    unpublishedOnHead: 2,
    unpublishedElsewhere: 0,
    stashes: 0,
    syncedAt: minutesFromNow(-60 * 24),
    remotes: [{ name: 'origin', url: 'git@github.example:ana/shop.git' }],
    operation: null,
    changed: 0,
    untracked: 0,
    unverifiable: 0,
    worktreeUnknown: null,
    ...facts,
  }
}

function installationReading(facts: Partial<InstallationReading> = {}): InstallationReading {
  return {
    id: 'r-notes',
    subject: notes,
    referenceId: 'notes-installed',
    machineId: 'laptop',
    path: 'C:\\Program Files\\Atlas Notes\\atlas-notes.exe',
    source: 'read',
    reader: 'DevHQ installation reader 1',
    agent: 'claude-code',
    observedAt: minutesFromNow(-30),
    recordedAt: minutesFromNow(-30),
    outcome: 'read',
    detail: null,
    storedAt: null,
    sha256: '5f2d9a1c'.repeat(8),
    sizeBytes: 23_620_096,
    modifiedAt: minutesFromNow(-60 * 24 * 2),
    productVersion: '1.5.0+3f9c2ab',
    fileVersion: '1.5.0.0',
    productName: 'Atlas Notes',
    versionUnknown: null,
    processes: 0,
    processesUnexamined: 3,
    processesUnknown: null,
    ...facts,
  }
}

const shopCopy: LocalCopy = {
  referenceId: 'shop-laptop',
  subject: shop,
  machineId: 'laptop',
  path: 'C:\\src\\shop',
  latest: copyReading(),
  movedSinceRead: false,
  repository: 'matches',
  localWork: true,
}
// Never read, on this computer.
const shopSpare: LocalCopy = {
  ...shopCopy,
  referenceId: 'shop-spare',
  path: 'D:\\spare\\shop',
  latest: null,
  repository: null,
  localWork: false,
}
const notesInstalled: Installation = {
  referenceId: 'notes-installed',
  subject: notes,
  machineId: 'laptop',
  path: 'C:\\Program Files\\Atlas Notes\\atlas-notes.exe',
  latest: installationReading(),
  movedSinceRead: false,
  version: 'differs',
  build: { sameSince: minutesFromNow(-30), readings: 1, previous: null, complete: true },
  againstCopy: null,
}

const reference = (id: string, kind: 'local_copy' | 'installation', target: string) => ({
  id,
  kind,
  target,
  label: null,
  machineId: 'laptop',
})

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
        reference('shop-laptop', 'local_copy', 'C:\\src\\shop'),
        reference('shop-spare', 'local_copy', 'D:\\spare\\shop'),
      ],
    },
    {
      id: 'notes',
      name: 'Atlas Notes',
      description: '',
      lifecycle: 'production',
      version: '1.4.0',
      components: [],
      references: [
        reference(
          'notes-installed',
          'installation',
          'C:\\Program Files\\Atlas Notes\\atlas-notes.exe',
        ),
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
  ],
  sharedRepositories: [],
  localCopies: [shopSpare, shopCopy],
  installations: [notesInstalled],
  observations: [],
  incidents: [],
  events: [],
  httpChecks: [],
  changes: [],
  changesSince: null,
  automatedHistory: { limit: 200, truncated: [] },
}

function renderWith(value: InventoryContextValue, children: ReactNode) {
  return render(
    <MemoryRouter>
      <InventoryContext.Provider value={value}>{children}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

describe('Pending work in Overview', () => {
  it('ranks what the readings say is waiting, each with its reasons and when it was read', () => {
    renderWith(
      inventoryContext({ snapshot, thisWorkstation: laptop, now: testNow }),
      <PendingWorkOverview />,
    )
    const section = screen.getByRole('region', { name: /Pending work/ })
    const rows = within(section).getAllByRole('listitem')
    expect(rows).toHaveLength(3)
    // Unpublished commits before another version, and both before a copy never read.
    expect(rows[0]).toHaveTextContent('Shop')
    expect(rows[0]).toHaveTextContent('2 commits on no remote')
    expect(rows[0]).toHaveTextContent('Read 2 hours ago')
    expect(rows[1]).toHaveTextContent('Atlas Notes')
    expect(rows[1]).toHaveTextContent('The file declared 1.5.0+3f9c2ab, the inventory 1.4.0')
    expect(rows[1]).toHaveTextContent('asked by claude-code')
    expect(rows[2]).toHaveTextContent('D:\\spare\\shop')
    expect(rows[2]).toHaveTextContent('Never read on this computer')
    expect(section).toHaveTextContent('Not health: none of it is in Needs attention.')
  })

  it('says when the installed build is not what the local copy held', () => {
    const behind = {
      ...notesInstalled,
      version: 'matches' as const,
      againstCopy: {
        state: 'other_commit' as const,
        copyReferenceId: 'notes-copy',
        copyReadingId: 'copy-reading',
        installedCommit: '3f9c2ab',
        copyHead: '9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a07',
      },
    }
    renderWith(
      inventoryContext({
        snapshot: { ...snapshot, localCopies: [], installations: [behind] },
        thisWorkstation: laptop,
        now: testNow,
      }),
      <PendingWorkOverview />,
    )
    expect(
      screen.getByText(
        "The installed build is commit 3f9c2ab, the local copy's HEAD was 9b8a7c6d5e4f",
      ),
    ).toBeInTheDocument()
  })

  it('says a reading older than three days is worth renewing, with what it held', () => {
    const old = { ...shopCopy, latest: copyReading({ observedAt: minutesFromNow(-60 * 24 * 4) }) }
    renderWith(
      inventoryContext({
        snapshot: { ...snapshot, localCopies: [old], installations: [] },
        thisWorkstation: laptop,
        now: testNow,
      }),
      <PendingWorkOverview />,
    )
    expect(
      screen.getByText('2 commits on no remote · read more than 3 days ago'),
    ).toBeInTheDocument()
  })

  it('never reassures when nothing is declared, and says nothing waits when readings agree', () => {
    const { unmount } = renderWith(
      inventoryContext({
        snapshot: { ...snapshot, localCopies: [], installations: [] },
        thisWorkstation: laptop,
        now: testNow,
      }),
      <PendingWorkOverview />,
    )
    expect(screen.getByText(/No local copy or installation is declared/)).toBeInTheDocument()
    unmount()
    const clean = { ...shopCopy, latest: copyReading({ unpublishedOnHead: 0 }), localWork: false }
    renderWith(
      inventoryContext({
        snapshot: { ...snapshot, localCopies: [clean], installations: [] },
        thisWorkstation: laptop,
        now: testNow,
      }),
      <PendingWorkOverview />,
    )
    expect(
      screen.getByText('No reading says work is waiting in a local copy or an installation.'),
    ).toBeInTheDocument()
  })

  it('says it beside the ecosystem status, which stays about health', () => {
    renderWith(
      inventoryContext({ snapshot, thisWorkstation: laptop, now: testNow }),
      <RealOverviewPage />,
    )
    const status = screen.getByRole('region', { name: 'Ecosystem status' })
    expect(status).toHaveTextContent('No recorded problems, but coverage is incomplete.')
    expect(status).toHaveTextContent(
      'Apart from health, 3 things are waiting in local copies and installations',
    )
  })
})

function SelectRealInventory() {
  const { setMode } = useInventory()
  return <button onClick={() => setMode('real')}>Select real inventory</button>
}

describe('the read when the window opens', () => {
  afterEach(() => vi.restoreAllMocks())

  function open(workstation: ThisWorkstation) {
    vi.spyOn(inventoryApi, 'watchHttpMonitor').mockResolvedValue()
    const list = vi.spyOn(inventoryApi, 'list').mockResolvedValue(snapshot)
    vi.spyOn(inventoryApi, 'thisWorkstation').mockResolvedValue(workstation)
    const readCopies = vi.spyOn(inventoryApi, 'readCopies')
    const readInstallations = vi.spyOn(inventoryApi, 'readInstallations')
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <PendingWorkOverview />
        </InventoryProvider>
      </MemoryRouter>,
    )
    return { list, readCopies, readInstallations }
  }

  it("reads this computer's copies, then its installations, once, and says what it read", async () => {
    const { list, readCopies, readInstallations } = open(laptop)
    readCopies.mockResolvedValue({
      readings: [copyReading()],
      failures: [
        {
          referenceId: 'shop-spare',
          subject: shop,
          path: 'D:\\spare\\shop',
          message: 'The reader did not answer within 30 s.',
        },
      ],
      thisWorkstation: laptop,
    })
    readInstallations.mockResolvedValue({
      readings: [installationReading()],
      failures: [],
      thisWorkstation: laptop,
    })
    const user = userEvent.setup()
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    expect(
      await screen.findByText(
        'When DevHQ opened, it read 1 local copy (1 could not be read and kept nothing); read 1 installation.',
      ),
    ).toBeInTheDocument()
    expect(readCopies).toHaveBeenCalledExactlyOnceWith(null)
    expect(readInstallations).toHaveBeenCalledExactlyOnceWith(null)
    expect(readCopies.mock.invocationCallOrder[0]).toBeLessThan(
      readInstallations.mock.invocationCallOrder[0]!,
    )
    // Each read reloads the inventory; nothing reads again by itself after that.
    expect(list).toHaveBeenCalledTimes(3)
    await new Promise((resolve) => setTimeout(resolve, 50))
    expect(readCopies).toHaveBeenCalledTimes(1)
  })

  it('says a refused read read nothing, and goes on with the installations', async () => {
    const { readCopies, readInstallations } = open(laptop)
    readCopies.mockRejectedValue({ code: 'read_error', message: 'The inventory is busy' })
    readInstallations.mockResolvedValue({ readings: [], failures: [], thisWorkstation: laptop })
    const user = userEvent.setup()
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    expect(
      await screen.findByText(
        'When DevHQ opened, it failed to read the local copies (The inventory is busy. Nothing was read); read 0 installations.',
      ),
    ).toBeInTheDocument()
  })

  it('reads nothing while DevHQ does not know which workstation this computer is', async () => {
    const { list, readCopies, readInstallations } = open({
      ...laptop,
      resourceId: null,
      chosenAt: null,
      problem: 'not_chosen',
      message: 'No workstation has been chosen as this computer yet.',
    })
    const user = userEvent.setup()
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    await waitFor(() => expect(list).toHaveBeenCalledTimes(1))
    await screen.findByRole('region', { name: /Pending work/ })
    expect(readCopies).not.toHaveBeenCalled()
    expect(readInstallations).not.toHaveBeenCalled()
  })
})
