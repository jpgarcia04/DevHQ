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
import { inventoryApi } from './inventory-api'
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

  it('says that an inventory in an older schema needs upgrading, and never asks again', async () => {
    const user = userEvent.setup()
    const list = vi.spyOn(inventoryApi, 'list').mockRejectedValue({
      code: 'schema_outdated',
      message:
        '/home/you/.local/share/com.devhq.desktop/inventory.sqlite3: A DevHQ inventory that declares schema version 6, older than this DevHQ’s (12).',
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
      await screen.findByRole('heading', { name: 'Inventory needs a schema upgrade' }),
    ).toBeInTheDocument()
    expect(screen.getByText(/declares schema version 6/)).toHaveTextContent(
      /Settings shows where the file and its backups folder are/,
    )
    // Not presented as empty, and nothing to retry: DevHQ decided it when it started.
    expect(screen.queryByText(/Nothing is registered yet/)).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Retry' })).not.toBeInTheDocument()
    expect(list).toHaveBeenCalledTimes(1)
    list.mockRestore()
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
