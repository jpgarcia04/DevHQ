import { describe, expect, it, vi } from 'vitest'
import { act, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type { ReactNode } from 'react'
import { InventoryContext, useInventory, type InventoryContextValue } from './inventory-context'
import { ApplicationForm } from './application-form'
import { RealApplicationDetailPage } from './real-application-detail-page'
import { RealApplicationsPage } from './real-applications-page'
import { RealInfrastructurePage } from './real-infrastructure-page'
import { RealOverviewPage } from './real-overview-page'
import { InventoryProvider } from './inventory-provider'
import { inventoryApi, type BeforeUpgrade, type UpgradeInspection } from './inventory-api'
import { inventoryContext as context } from './test-context'

function renderWithInventory(ui: ReactNode, value: InventoryContextValue) {
  return render(
    <MemoryRouter>
      <InventoryContext.Provider value={value}>{ui}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

function SelectRealInventory() {
  const { setMode } = useInventory()
  return <button onClick={() => setMode('real')}>Select real inventory</button>
}

describe('real inventory UI', () => {
  it('submits a component linked to the selected resource', async () => {
    const user = userEvent.setup()
    const value = context()
    const onDone = vi.fn()
    renderWithInventory(<ApplicationForm onDone={onDone} />, value)
    await user.type(screen.getByRole('textbox', { name: 'Name' }), 'Service One')
    await user.click(screen.getByRole('button', { name: 'Add component' }))
    await user.type(screen.getByRole('textbox', { name: 'Component name' }), 'API')
    await user.selectOptions(screen.getByRole('combobox', { name: 'Resource' }), 'host-1')
    await user.click(screen.getByRole('button', { name: 'Save application' }))
    expect(value.saveApplication).toHaveBeenCalledWith(
      expect.objectContaining({
        name: 'Service One',
        lifecycle: 'development',
        components: [{ name: 'API', kind: 'backend', resourceId: 'host-1' }],
      }),
    )
    expect(onDone).toHaveBeenCalledOnce()
  })

  it('leaves names and versions out of the spell checker, not descriptions', async () => {
    const user = userEvent.setup()
    renderWithInventory(<ApplicationForm onDone={vi.fn()} />, context())
    await user.click(screen.getByRole('button', { name: 'Add component' }))
    for (const name of ['Name', 'Version', 'Component name'])
      expect(screen.getByRole('textbox', { name })).toHaveAttribute('spellcheck', 'false')
    expect(screen.getByRole('textbox', { name: 'Description' })).not.toHaveAttribute('spellcheck')
  })

  it('keeps a failed inventory request distinct from an empty or healthy inventory', async () => {
    const user = userEvent.setup()
    const list = vi.spyOn(inventoryApi, 'list').mockRejectedValue({
      code: 'database_error',
      message: 'Inventory database error: disk unavailable',
    })
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <RealOverviewPage />
        </InventoryProvider>
      </MemoryRouter>,
    )
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    expect(
      await screen.findByRole('heading', { name: 'Inventory unavailable' }),
    ).toBeInTheDocument()
    expect(screen.getByText(/disk unavailable/)).toBeInTheDocument()
    expect(screen.queryByText('0')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Retry' }))
    expect(list).toHaveBeenCalledTimes(2)
    list.mockRestore()
  })

  it('says why no inventory was created beside files another one left, and never asks again', async () => {
    const user = userEvent.setup()
    const list = vi.spyOn(inventoryApi, 'list').mockRejectedValue({
      code: 'leftover_files',
      message:
        'There is no DevHQ inventory at /home/you/.local/share/com.devhq.desktop/inventory.sqlite3, yet files SQLite keeps beside a database are there (/home/you/.local/share/com.devhq.desktop/inventory.sqlite3-journal).',
    })
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <RealOverviewPage />
        </InventoryProvider>
      </MemoryRouter>,
    )
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    expect(
      await screen.findByRole('heading', { name: 'Inventory unavailable' }),
    ).toBeInTheDocument()
    expect(screen.getByText(/inventory\.sqlite3-journal/)).toHaveTextContent(
      /looks at the folder again the next time it starts/,
    )
    expect(screen.queryByText(/Nothing is registered yet/)).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument()
    expect(list).toHaveBeenCalledTimes(1)
    list.mockRestore()
  })

  const outdatedRefusal = {
    code: 'schema_outdated',
    message:
      '/home/you/.local/share/com.devhq.desktop/inventory.sqlite3: A DevHQ inventory that declares schema version 6, older than this DevHQ’s (12).',
  }
  const outdated: UpgradeInspection = {
    database: '/home/you/.local/share/com.devhq.desktop/inventory.sqlite3',
    storedAt: null,
    state: 'outdated',
    storedVersion: 6,
    version: 12,
    message: 'A DevHQ inventory in schema version 6, older than this DevHQ’s (12).',
    upgradable: true,
    problems: [],
    tables: [
      { table: 'applications', rows: 1 },
      { table: 'resources', rows: 2 },
    ],
    fingerprint: 'f'.repeat(64),
    backups: '/home/you/.local/share/com.devhq.desktop/backups',
  }
  const backupMade: BeforeUpgrade = {
    name: 'inventory-20260930T120000.000Z-v6.sqlite3',
    path: '/home/you/.local/share/com.devhq.desktop/backups/inventory-20260930T120000.000Z-v6.sqlite3',
    storedAt: null,
    manifest:
      '/home/you/.local/share/com.devhq.desktop/backups/inventory-20260930T120000.000Z-v6.sqlite3.json',
    takenAt: '2026-09-30T12:00:00.000Z',
    sizeBytes: 65_536,
    sha256: 'a'.repeat(64),
    schemaVersion: 6,
    integrityCheck: 'ok',
    tables: outdated.tables,
  }
  const upgradedInventory = {
    ...context().snapshot!,
    resources: [
      {
        id: 'host-1',
        name: 'Kept through the upgrade',
        kind: 'server' as const,
        description: '',
        operatingSystem: 'Linux',
        references: [],
      },
    ],
  }

  /** The real provider over a native process that refuses the inventory as outdated. */
  function outdatedWindow() {
    const list = vi.spyOn(inventoryApi, 'list').mockRejectedValue(outdatedRefusal)
    const inspect = vi.spyOn(inventoryApi, 'inspectUpgrade').mockResolvedValue(outdated)
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <RealOverviewPage />
        </InventoryProvider>
      </MemoryRouter>,
    )
    return { list, inspect }
  }

  it('offers to back up and upgrade an inventory in an older schema, and serves it once upgraded', async () => {
    const user = userEvent.setup()
    const { list, inspect } = outdatedWindow()
    // The upgrade stops once the backup is verified, until the test lets it go on.
    let goOn = () => {}
    const upgrade = vi
      .spyOn(inventoryApi, 'upgradeInventory')
      .mockImplementation(async (_from, _fingerprint, onStep) => {
        onStep({ step: 'checking' })
        onStep({ step: 'backingUp' })
        onStep({ step: 'backedUp', backup: backupMade })
        await new Promise<void>((resolve) => (goOn = resolve))
        onStep({ step: 'upgrading' })
        onStep({ step: 'verifying' })
        list.mockResolvedValue(upgradedInventory)
        return { from: 6, to: 12, backup: backupMade, tablesKept: 2, rowsKept: 3 }
      })
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    expect(
      await screen.findByRole('heading', { name: 'Inventory needs a schema upgrade' }),
    ).toBeInTheDocument()
    // What it is, as the native process found it without changing it.
    expect(await screen.findByText(/is a DevHQ inventory in schema version 6/)).toHaveTextContent(
      'this DevHQ reads version 12',
    )
    expect(screen.getByText(/It holds 3 rows in 2 tables/)).toBeInTheDocument()
    expect(screen.getByText(/with a manifest beside it/)).toHaveTextContent(outdated.backups)
    expect(screen.getByText(/same disk as the inventory/)).toBeInTheDocument()
    // Not presented as empty, no Retry, and no way to upgrade without the backup.
    expect(screen.queryByText(/Nothing is registered yet/)).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument()
    expect(screen.getAllByRole('button').map((button) => button.textContent)).toStrictEqual([
      'Select real inventory',
      'Back up and upgrade',
    ])
    expect(upgrade).not.toHaveBeenCalled()

    await user.click(screen.getByRole('button', { name: 'Back up and upgrade' }))
    expect(upgrade).toHaveBeenCalledWith(6, outdated.fingerprint, expect.any(Function))
    const progress = await screen.findByRole('status', { name: 'Upgrade' })
    await waitFor(() =>
      expect(progress).toHaveTextContent('✓ Backup verified and named, with its manifest'),
    )
    expect(progress).toHaveTextContent(backupMade.path)
    expect(progress).toHaveTextContent('· Upgrading')
    expect(screen.queryByRole('button', { name: 'Back up and upgrade' })).not.toBeInTheDocument()

    await act(async () => goOn())
    // Served: the inventory, with what the upgrade did above it.
    expect((await screen.findAllByText('Kept through the upgrade')).length).toBeGreaterThan(0)
    const done = screen.getByRole('region', { name: 'Inventory upgraded' })
    expect(done).toHaveTextContent('From schema version 6 to 12. All 3 rows in 2 tables')
    expect(done).toHaveTextContent(backupMade.path)
    expect(done).toHaveTextContent(backupMade.manifest)
    expect(done).toHaveTextContent(backupMade.sha256)
    expect(inspect).toHaveBeenCalledTimes(1)
    await user.click(within(done).getByRole('button', { name: 'Dismiss' }))
    expect(screen.queryByRole('region', { name: 'Inventory upgraded' })).not.toBeInTheDocument()
    vi.restoreAllMocks()
  })

  it('says why it does not upgrade an inventory it cannot vouch for, and offers nothing', async () => {
    const user = userEvent.setup()
    const { inspect } = outdatedWindow()
    inspect.mockResolvedValue({
      ...outdated,
      upgradable: false,
      fingerprint: null,
      problems: ['SQLite’s integrity check found it damaged: row 3 missing from index'],
    })
    const upgrade = vi.spyOn(inventoryApi, 'upgradeInventory')
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    expect(
      await screen.findByText(/DevHQ does not upgrade it: it never changes what it cannot vouch/),
    ).toBeInTheDocument()
    expect(screen.getByText(/row 3 missing from index/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Back up and upgrade' })).not.toBeInTheDocument()
    expect(upgrade).not.toHaveBeenCalled()
    vi.restoreAllMocks()
  })

  it('says what a failed upgrade did and where its backup is, and looks at the inventory again', async () => {
    const user = userEvent.setup()
    const { inspect } = outdatedWindow()
    vi.spyOn(inventoryApi, 'upgradeInventory').mockImplementation(async (_f, _p, onStep) => {
      onStep({ step: 'checking' })
      onStep({ step: 'backingUp' })
      onStep({ step: 'backedUp', backup: backupMade })
      onStep({ step: 'upgrading' })
      throw {
        code: 'upgrade_failed',
        message: `migration 012 failed: disk I/O error. Nothing of the upgrade was committed: the inventory holds what it held before, in schema version 6. The backup made before it is kept at ${backupMade.path}.`,
      }
    })
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    await user.click(await screen.findByRole('button', { name: 'Back up and upgrade' }))
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('The upgrade did not happen.')
    expect(alert).toHaveTextContent('Nothing of the upgrade was committed')
    expect(alert).toHaveTextContent(backupMade.manifest)
    // Looked at again: still outdated, so the person can try again.
    expect(await screen.findByRole('button', { name: 'Back up and upgrade' })).toBeInTheDocument()
    expect(inspect).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole('region', { name: 'Inventory upgraded' })).not.toBeInTheDocument()
    vi.restoreAllMocks()
  })

  it('titles an application’s page while the inventory loads and when it cannot be read', () => {
    const view = (value: InventoryContextValue) => (
      <MemoryRouter initialEntries={['/applications/shop']}>
        <InventoryContext.Provider value={value}>
          <Routes>
            <Route path="/applications/:applicationId" element={<RealApplicationDetailPage />} />
          </Routes>
        </InventoryContext.Provider>
      </MemoryRouter>
    )
    const { rerender } = render(view(context({ snapshot: null, loadState: 'loading' })))
    expect(screen.getByRole('heading', { level: 1, name: 'Application' })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Loading local inventory…')
    expect(screen.getByRole('link', { name: 'All applications' })).toHaveAttribute(
      'href',
      '/applications',
    )

    const reload = vi.fn(async () => false)
    rerender(
      view(context({ snapshot: null, loadState: 'error', error: 'Disk unavailable', reload })),
    )
    expect(screen.getByRole('heading', { level: 1, name: 'Application' })).toBeInTheDocument()
    expect(
      screen.getByRole('heading', { level: 2, name: 'Inventory unavailable' }),
    ).toBeInTheDocument()
    expect(screen.getByText('Disk unavailable')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
  })

  it('shows no observed health for real inventory', () => {
    const value = context()
    renderWithInventory(
      <main>
        <RealOverviewPage />
      </main>,
      value,
    )
    const main = within(screen.getByRole('main'))
    expect(main.getByText('Not observed')).toBeInTheDocument()
    expect(main.getByText('No recorded problems, but coverage is incomplete.')).toBeInTheDocument()
    expect(
      main.getByText('1 of 1 system has never been observed. DevHQ does not assume it is healthy.'),
    ).toBeInTheDocument()
    expect(main.queryByText('Healthy')).not.toBeInTheDocument()
  })

  it('counts components, linked resources and applications in the right number', async () => {
    const base = context()
    const application = (id: string, name: string, resourceIds: (string | null)[]) => ({
      id,
      name,
      description: '',
      lifecycle: 'production' as const,
      version: null,
      components: resourceIds.map((resourceId, index) => ({
        id: `${id}-${index}`,
        name: `part ${index}`,
        kind: 'backend' as const,
        resourceId,
      })),
      references: [],
    })
    renderWithInventory(
      <RealApplicationsPage />,
      context({
        snapshot: {
          ...base.snapshot!,
          resources: [
            ...base.snapshot!.resources,
            {
              id: 'host-2',
              name: 'Host Two',
              kind: 'server',
              description: '',
              operatingSystem: '',
              references: [],
            },
          ],
          applications: [
            application('one', 'One', ['host-1']),
            application('two', 'Two', ['host-1', 'host-2', null]),
          ],
        },
      }),
    )
    expect(screen.getByRole('link', { name: 'Open One' })).toHaveTextContent(
      '1 component · 1 linked resource',
    )
    expect(screen.getByRole('link', { name: 'Open Two' })).toHaveTextContent(
      '3 components · 2 linked resources',
    )
    expect(screen.getByText('2 applications shown')).toBeInTheDocument()
    await userEvent.setup().type(screen.getByPlaceholderText('Find an application…'), 'Two')
    expect(screen.getByText('1 application shown')).toBeInTheDocument()
  })

  it('focuses a linked resource in the real inventory', async () => {
    render(
      <MemoryRouter initialEntries={['/infrastructure?resource=host-1']}>
        <InventoryContext.Provider value={context()}>
          <RealInfrastructurePage />
        </InventoryContext.Provider>
      </MemoryRouter>,
    )
    await waitFor(() => expect(screen.getByRole('region', { name: 'Host One' })).toHaveFocus())
  })

  it('never pulls focus back to the linked resource when a newer snapshot arrives', async () => {
    const value = context()
    const snapshot = {
      ...value.snapshot!,
      resources: [
        ...value.snapshot!.resources,
        {
          id: 'host-2',
          name: 'Host Two',
          kind: 'server' as const,
          description: '',
          operatingSystem: '',
          references: [],
        },
      ],
    }
    const view = (current: InventoryContextValue) => (
      <MemoryRouter initialEntries={['/infrastructure?resource=host-1']}>
        <InventoryContext.Provider value={current}>
          <RealInfrastructurePage />
        </InventoryContext.Provider>
      </MemoryRouter>
    )
    const { rerender } = render(view(context({ snapshot })))
    await waitFor(() => expect(screen.getByRole('region', { name: 'Host One' })).toHaveFocus())
    const elsewhere = within(screen.getByRole('region', { name: 'Host Two' })).getByRole('button', {
      name: 'Edit',
    })
    act(() => elsewhere.focus())
    // A save or a supervised run brings a new snapshot while the person works elsewhere.
    rerender(view(context({ snapshot: { ...snapshot } })))
    await act(() => new Promise((resolve) => window.setTimeout(resolve, 20)))
    expect(elsewhere).toHaveFocus()
  })
})
