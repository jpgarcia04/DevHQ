import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type { ReactNode } from 'react'
import {
  againstCopyText,
  buildText,
  installationsOverview,
  processesText,
  readAllInstallations,
  sizeText,
  versionText,
} from './installations'
import { InstallationsOverview } from './installations-overview'
import type {
  Installation,
  InstallationReading,
  InventorySnapshot,
  ThisWorkstation,
} from './inventory-api'
import { InventoryContext, type InventoryContextValue } from './inventory-context'
import { RealApplicationDetailPage } from './real-application-detail-page'
import { inventoryContext, minutesFromNow } from './test-context'

const notes = { type: 'application' as const, id: 'notes' }
const old = { type: 'application' as const, id: 'old' }
const path = 'C:\\Program Files\\Atlas Notes\\atlas-notes.exe'

const laptop: ThisWorkstation = {
  resourceId: 'laptop',
  chosenAt: minutesFromNow(-60 * 24),
  problem: null,
  message: null,
  file: 'C:\\Users\\you\\DevHQ\\this-workstation.json',
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

function reading(id: string, facts: Partial<InstallationReading> = {}): InstallationReading {
  return {
    id,
    subject: notes,
    referenceId: 'notes-installed',
    machineId: 'laptop',
    path,
    source: 'read',
    reader: 'DevHQ installation reader 1',
    agent: null,
    observedAt: minutesFromNow(-120),
    recordedAt: minutesFromNow(-120),
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
    processes: 2,
    processesUnexamined: 41,
    processesUnknown: null,
    ...facts,
  }
}

function installation(referenceId: string, facts: Partial<Installation> = {}): Installation {
  return {
    referenceId,
    subject: notes,
    machineId: 'laptop',
    path,
    latest: null,
    movedSinceRead: false,
    version: null,
    build: null,
    againstCopy: null,
    ...facts,
  }
}

const read = reading('reading-new')
const differs = installation('notes-installed', {
  latest: read,
  version: 'differs',
  build: {
    sameSince: minutesFromNow(-120),
    readings: 1,
    previous: {
      observedAt: minutesFromNow(-60 * 24 * 3),
      sha256: 'a81c04e7'.repeat(8),
      productVersion: '1.4.0+11d2e4f',
    },
    complete: true,
  },
})
const missing = installation('notes-portable', {
  path: 'D:\\Portable\\atlas-notes.exe',
  latest: reading('reading-gone', {
    referenceId: 'notes-portable',
    path: 'D:\\Portable\\atlas-notes.exe',
    outcome: 'missing',
    detail: 'Nothing is at this path.',
    sha256: null,
    sizeBytes: null,
    modifiedAt: null,
    productVersion: null,
    fileVersion: null,
    productName: null,
    processes: null,
    processesUnexamined: null,
  }),
})
const neverHere = installation('notes-beta', { path: 'C:\\Beta\\atlas-notes.exe' })
const neverElsewhere = installation('notes-desktop', {
  machineId: 'desktop',
  path: '/opt/atlas/atlas-notes',
})
const archivedOne = installation('old-installed', {
  subject: old,
  path: 'C:\\Old\\old.exe',
  latest: reading('reading-old', { subject: old, referenceId: 'old-installed' }),
  version: 'differs',
})
const all = [differs, missing, neverHere, neverElsewhere, archivedOne]

const reference = (id: string, target: string, machineId: string) => ({
  id,
  kind: 'installation' as const,
  target,
  label: null,
  machineId,
})

const snapshot: InventorySnapshot = {
  applications: [
    {
      id: 'notes',
      name: 'Atlas Notes',
      description: '',
      lifecycle: 'production',
      version: '1.4.0',
      components: [],
      references: [
        reference('notes-installed', path, 'laptop'),
        reference('notes-portable', 'D:\\Portable\\atlas-notes.exe', 'laptop'),
        reference('notes-beta', 'C:\\Beta\\atlas-notes.exe', 'laptop'),
        reference('notes-desktop', '/opt/atlas/atlas-notes', 'desktop'),
      ],
    },
    {
      id: 'old',
      name: 'Old Notes',
      description: '',
      lifecycle: 'archived',
      version: '0.9',
      components: [],
      references: [reference('old-installed', 'C:\\Old\\old.exe', 'laptop')],
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
  ],
  sharedRepositories: [],
  localCopies: [],
  installations: all,
  githubRepositories: [],
  observations: [],
  incidents: [],
  events: [],
  httpChecks: [],
  changes: [],
  changesSince: null,
  automatedHistory: { limit: 200, truncated: [] },
}

function renderWith(value: InventoryContextValue, children: ReactNode, at = '/') {
  return render(
    <MemoryRouter initialEntries={[at]}>
      <InventoryContext.Provider value={value}>{children}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

function notesDetail(value: InventoryContextValue) {
  return renderWith(
    value,
    <Routes>
      <Route path="/applications/:applicationId" element={<RealApplicationDetailPage />} />
    </Routes>,
    '/applications/notes',
  )
}

describe('what the window says of installations', () => {
  it('reads all of this workstation but archived applications, and nothing while unknown', () => {
    expect(readAllInstallations(snapshot, laptop).map((item) => item.referenceId)).toEqual([
      'notes-installed',
      'notes-portable',
      'notes-beta',
    ])
    expect(readAllInstallations(snapshot, notChosen)).toEqual([])
    expect(readAllInstallations(snapshot, null)).toEqual([])
  })

  it('lists what the agent gate lists, without archived applications', () => {
    const ids = (items: Installation[]) => items.map((item) => item.referenceId)
    const here = installationsOverview(snapshot, laptop)
    expect(ids(here.versionDiffers)).toEqual(['notes-installed'])
    expect(ids(here.notFound)).toEqual(['notes-portable'])
    expect(ids(here.neverReadHere)).toEqual(['notes-beta'])
    expect(ids(here.neverReadElsewhere)).toEqual(['notes-desktop'])
    // Without a known workstation, none is «here».
    const unknown = installationsOverview(snapshot, notChosen)
    expect(ids(unknown.neverReadHere)).toEqual([])
    expect(ids(unknown.neverReadElsewhere)).toEqual(['notes-beta', 'notes-desktop'])
  })

  it('says both versions, and never which one is right', () => {
    expect(versionText(differs, '1.4.0')).toBe(
      'The inventory declares version 1.4.0; the file declared 1.5.0+3f9c2ab. One of the two is out of date.',
    )
    expect(versionText({ ...differs, version: 'matches' }, '1.5.0')).toBe(
      'The inventory declares version 1.5.0, the one the file declared.',
    )
    expect(versionText({ ...differs, version: 'undeclared' }, null)).toBe(
      'The file declared 1.5.0+3f9c2ab; the inventory declares no version to compare it with.',
    )
    expect(versionText({ ...differs, version: 'unknown' }, '1.4.0')).toBe(
      "The file's version is unknown, so DevHQ cannot say whether it was the declared 1.4.0.",
    )
    expect(versionText(neverHere, '1.4.0')).toBeNull()
  })

  it('says how the build compares with a local copy, and never more than it knows', () => {
    const against = {
      state: 'other_commit' as const,
      copyReferenceId: 'notes-copy',
      copyReadingId: 'copy-reading',
      installedCommit: '3f9c2ab',
      copyHead: '9b8a7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a07',
    }
    expect(againstCopyText(against)).toBe(
      "The file declared commit 3f9c2ab; HEAD was on 9b8a7c6d5e4f in the local copy's newest reading. What is installed is not what the copy held: DevHQ does not say whether it is older or newer.",
    )
    expect(againstCopyText({ ...against, state: 'head' })).toMatch(/the one HEAD was on/)
    expect(againstCopyText({ ...against, state: 'newer_commits' })).toMatch(
      /may or may not be in the installed build/,
    )
  })

  it('says between which two readings the build changed, and never when it was installed', () => {
    const changed = buildText(differs.build!)
    expect(changed).toMatch(/^Another build was there on .+ \(it declared 1\.4\.0\+11d2e4f\)/)
    expect(changed).toMatch(/it changed between those two readings\. DevHQ does not know when\.$/)
    const since = { sameSince: minutesFromNow(-600), previous: null }
    expect(buildText({ ...since, readings: 4, complete: true })).toMatch(
      /^The same build in all 4 readings, since /,
    )
    // The look back stopped: the build is the same for at least that long, and it says so.
    expect(buildText({ ...since, readings: 201, complete: false })).toMatch(
      /^The same build in the last 201 readings, since at least .+ DevHQ looked no further back\.$/,
    )
    expect(buildText({ ...since, readings: 1, complete: true })).toBe(
      'Read once: there is no earlier reading to compare its build with.',
    )
  })

  it('says how many processes were running, in the past, and what it could not look at', () => {
    expect(processesText(read)).toBe(
      '2 processes were running from it; the system did not let DevHQ look at 41 other processes named like it',
    )
    expect(processesText(reading('one', { processes: 1, processesUnexamined: 0 }))).toBe(
      '1 process was running from it',
    )
    expect(processesText(reading('none', { processes: 0, processesUnexamined: 1 }))).toBe(
      'None was running from it; the system did not let DevHQ look at 1 other process named like it',
    )
    expect(
      processesText(
        reading('unknown', {
          processes: null,
          processesUnexamined: null,
          processesUnknown: 'Windows did not list its processes.',
        }),
      ),
    ).toBe('Unknown: Windows did not list its processes.')
    expect(sizeText(23_620_096)).toBe('22.5 MB (23,620,096 bytes)')
    expect(sizeText(512)).toBe('512 bytes')
  })
})

describe('an installation under its reference', () => {
  it('shows what the newest reading found, in the past, with who asked', () => {
    const agent = { ...differs, latest: reading('reading-new', { agent: 'claude-code' }) }
    notesDetail(
      inventoryContext({
        snapshot: { ...snapshot, installations: [agent, missing, neverHere, neverElsewhere] },
        thisWorkstation: laptop,
      }),
    )
    const item = within(screen.getByText(path).closest('li')!)
    expect(item.getByText('Automated · Local read · asked by claude-code')).toBeInTheDocument()
    const facts = within(item.getByLabelText('What the reading found'))
    expect(facts.getByText('1.5.0+3f9c2ab')).toBeInTheDocument()
    expect(facts.getByText('5f2d9a1c'.repeat(8))).toBeInTheDocument()
    expect(facts.getByText(/^2 processes were running from it/)).toBeInTheDocument()
    expect(item.getByText(/One of the two is out of date\.$/)).toBeInTheDocument()
    expect(item.getByText(/it changed between those two readings/)).toBeInTheDocument()
    // Nothing says the application is open or running now.
    expect(item.queryByText(/is open|is running|are running/i)).not.toBeInTheDocument()
    expect(item.getByRole('button', { name: `Read ${path}` })).toBeEnabled()
    // Not found, never read, and one on another computer, each said as it is.
    expect(screen.getByText('Nothing is at this path.')).toBeInTheDocument()
    expect(
      screen.getAllByText('Never read: DevHQ knows nothing of what is installed here.'),
    ).toHaveLength(2)
    expect(screen.getByText(/reads it; this computer is/)).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: 'Read /opt/atlas/atlas-notes' }),
    ).not.toBeInTheDocument()
  })

  it('says where the system really kept the file, and why its version is unknown', () => {
    const redirected = {
      ...differs,
      version: 'unknown' as const,
      latest: reading('reading-new', {
        storedAt: 'C:\\Users\\you\\AppData\\Local\\Packages\\Example.App_abc\\LocalCache\\x.exe',
        productVersion: null,
        fileVersion: null,
        productName: null,
        versionUnknown: 'The file has no version resource.',
      }),
    }
    notesDetail(
      inventoryContext({
        snapshot: { ...snapshot, installations: [redirected] },
        thisWorkstation: laptop,
      }),
    )
    expect(screen.getByText(/The system kept this file at/)).toHaveTextContent(
      'Packages\\Example.App_abc\\LocalCache\\x.exe, not at the path declared',
    )
    expect(screen.getByText('Unknown: The file has no version resource.')).toBeInTheDocument()
    expect(
      screen.getByText(
        "The file's version is unknown, so DevHQ cannot say whether it was the declared 1.4.0.",
      ),
    ).toBeInTheDocument()
  })

  it('reads when asked, and says so when DevHQ could not read', async () => {
    const readInstallations = vi
      .fn<InventoryContextValue['readInstallations']>()
      .mockResolvedValueOnce({ readings: [read], failures: [], thisWorkstation: laptop })
      .mockResolvedValueOnce({
        readings: [],
        failures: [
          {
            referenceId: 'notes-installed',
            subject: notes,
            path,
            message: `Reading ${path} took longer than 30 s, so DevHQ stopped it.`,
          },
        ],
        thisWorkstation: laptop,
      })
    const user = userEvent.setup()
    notesDetail(
      inventoryContext({
        snapshot: { ...snapshot, installations: [differs] },
        thisWorkstation: laptop,
        readInstallations,
      }),
    )
    await user.click(screen.getByRole('button', { name: `Read ${path}` }))
    expect(readInstallations).toHaveBeenCalledWith(['notes-installed'])
    expect(await screen.findByText('Read and kept. What it found is above.')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: `Read ${path}` }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'took longer than 30 s, so DevHQ stopped it. Nothing was kept.',
    )
  })

  it('reads none while this computer is unknown, and says where to choose it', () => {
    notesDetail(
      inventoryContext({
        snapshot: { ...snapshot, installations: [differs] },
        thisWorkstation: notChosen,
      }),
    )
    expect(screen.getByText(/DevHQ reads only installations on this computer/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: `Read ${path}` })).not.toBeInTheDocument()
  })

  it('loads every reading only when opened, and marks a path no longer declared', async () => {
    const olderInstallationReadings = vi
      .fn<InventoryContextValue['olderInstallationReadings']>()
      .mockResolvedValue({
        readings: [
          read,
          reading('reading-elsewhere', { referenceId: 'removed', path: 'C:\\Gone\\notes.exe' }),
        ],
        more: true,
      })
    const user = userEvent.setup()
    notesDetail(
      inventoryContext({
        snapshot: { ...snapshot, installations: [differs] },
        thisWorkstation: laptop,
        olderInstallationReadings,
      }),
    )
    expect(olderInstallationReadings).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Show every installation reading' }))
    const history = within(
      await screen.findByRole('region', { name: 'Installation readings of Atlas Notes' }),
    )
    expect(await history.findAllByRole('listitem')).toHaveLength(2)
    expect(olderInstallationReadings).toHaveBeenCalledWith(notes, null)
    expect(history.getByText(/no longer declared by Atlas Notes/)).toBeInTheDocument()
    // The list is cut, and says so.
    expect(history.getByText('The newest 2 readings; older ones are kept.')).toBeInTheDocument()
    await user.click(history.getByRole('button', { name: 'Load 200 older readings' }))
    expect(olderInstallationReadings).toHaveBeenLastCalledWith(notes, 'reading-elsewhere')
  })
})

describe('the Installations panel', () => {
  it('lists the version that differs, what was not found and what was never read', () => {
    renderWith(inventoryContext({ snapshot, thisWorkstation: laptop }), <InstallationsOverview />)
    const panel = within(screen.getByRole('heading', { name: 'Installations' }).parentElement!)
    expect(panel.getByText('3 installations on this computer,')).toBeInTheDocument()
    expect(panel.getByText('Declared version is not the installed one (1)')).toBeInTheDocument()
    expect(
      panel.getByText('The inventory declares 1.4.0; the file declared 1.5.0+3f9c2ab.'),
    ).toBeInTheDocument()
    expect(panel.getByText('Not found where declared (1)')).toBeInTheDocument()
    expect(panel.getByText('Never read on this computer (1)')).toBeInTheDocument()
    // An archived application's installation is left out, as from Read all.
    expect(panel.queryByText('C:\\Old\\old.exe')).not.toBeInTheDocument()
    expect(panel.getByText(/1 installation on other computers was never read/)).toBeInTheDocument()
    expect(panel.getByText(/Not health: none of it is in Needs attention/)).toBeInTheDocument()
  })

  it('reads all of this computer and says which could not be read', async () => {
    const readInstallations = vi
      .fn<InventoryContextValue['readInstallations']>()
      .mockResolvedValue({
        readings: [read],
        failures: [
          {
            referenceId: 'notes-beta',
            subject: notes,
            path: 'C:\\Beta\\atlas-notes.exe',
            message: 'The reader stopped without an answer (exit code: 1).',
          },
        ],
        thisWorkstation: laptop,
      })
    const user = userEvent.setup()
    renderWith(
      inventoryContext({ snapshot, thisWorkstation: laptop, readInstallations }),
      <InstallationsOverview />,
    )
    await user.click(
      screen.getByRole('button', { name: 'Read all installations on this computer' }),
    )
    expect(readInstallations).toHaveBeenCalledWith(null)
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Atlas Notes: The reader stopped without an answer',
    )
    expect(
      screen.getByText('Read 1 installation; 1 could not be read and kept nothing.'),
    ).toBeInTheDocument()
  })

  it('reads nothing and says why while this computer is unknown, or nothing is declared', () => {
    const { unmount } = renderWith(
      inventoryContext({ snapshot, thisWorkstation: notChosen }),
      <InstallationsOverview />,
    )
    expect(
      screen.getByText('DevHQ does not know which workstation this computer is'),
    ).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: 'Read all installations on this computer' }),
    ).not.toBeInTheDocument()
    unmount()
    renderWith(
      inventoryContext({ snapshot: { ...snapshot, installations: [] }, thisWorkstation: laptop }),
      <InstallationsOverview />,
    )
    expect(screen.getByText(/No system declares an installation\./)).toBeInTheDocument()
  })
})
