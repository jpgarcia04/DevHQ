import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type { ReactNode } from 'react'
import { ApplicationForm } from './application-form'
import { deletionBlocker } from './deletion'
import type { InventorySnapshot } from './inventory-api'
import { InventoryContext, type InventoryContextValue } from './inventory-context'
import { RealApplicationDetailPage } from './real-application-detail-page'
import { RealInfrastructurePage } from './real-infrastructure-page'
import { ResourceForm } from './resource-form'
import { inventoryContext } from './test-context'

const monorepo = 'https://github.com/me/web'
const clone = 'C:\\src\\web'

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
        { id: 'shop-repo', kind: 'repository', target: monorepo, label: null, machineId: null },
        { id: 'shop-clone', kind: 'local_copy', target: clone, label: 'Work', machineId: 'laptop' },
      ],
    },
    {
      id: 'blog',
      name: 'Blog',
      description: '',
      lifecycle: 'development',
      version: null,
      components: [],
      references: [
        { id: 'blog-repo', kind: 'repository', target: monorepo, label: null, machineId: null },
      ],
    },
    {
      id: 'draft',
      name: 'Draft',
      description: '',
      lifecycle: 'development',
      version: null,
      components: [],
      references: [],
    },
  ],
  resources: [
    {
      id: 'laptop',
      name: 'Laptop',
      kind: 'workstation',
      description: '',
      operatingSystem: 'Windows 11',
      references: [],
    },
    {
      id: 'vps',
      name: 'VPS',
      kind: 'server',
      description: '',
      operatingSystem: 'Debian 13',
      // Its own logs are on it.
      references: [
        { id: 'vps-logs', kind: 'logs', target: '/var/log/syslog', label: null, machineId: 'vps' },
      ],
    },
  ],
  sharedRepositories: [
    {
      target: monorepo,
      systems: [
        { type: 'application', id: 'shop' },
        { type: 'application', id: 'blog' },
      ],
    },
  ],
  observations: [],
  incidents: [],
  events: [],
  httpChecks: [],
  changes: [],
  changesSince: null,
  automatedHistory: { limit: 200, truncated: [] },
}

function renderWith(ui: ReactNode, value: InventoryContextValue, at = '/') {
  return render(
    <MemoryRouter initialEntries={[at]}>
      <InventoryContext.Provider value={value}>{ui}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

const detail = (id: string, value = inventoryContext({ snapshot })) =>
  renderWith(
    <Routes>
      <Route path="/applications/:applicationId" element={<RealApplicationDetailPage />} />
    </Routes>,
    value,
    `/applications/${id}`,
  )

afterEach(() => vi.unstubAllGlobals())

describe('where each system is', () => {
  it('shows an application’s references with each path’s machine, and copies a target', async () => {
    const user = userEvent.setup()
    const writeText = vi.fn().mockResolvedValue(undefined)
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    detail('shop')
    const list = within(screen.getByRole('list', { name: 'References' }))
    const [repository, local] = list.getAllByRole('listitem')
    expect(repository).toHaveTextContent(`Repository${monorepo}Same repository as BlogCopy`)
    expect(within(repository!).getByRole('link', { name: 'Blog' })).toHaveAttribute(
      'href',
      '/applications/blog',
    )
    // The label names it for people; the target is what is copied.
    expect(local).toHaveTextContent(`Local copy · Work${clone}On LaptopCopy`)
    expect(within(local!).getByRole('link', { name: 'Laptop' })).toBeInTheDocument()
    await user.click(within(local!).getByRole('button', { name: `Copy ${clone}` }))
    expect(writeText).toHaveBeenCalledWith(clone)
    expect(within(local!).getByRole('status')).toHaveTextContent('Copied.')
    expect(within(repository!).getByRole('status')).toBeEmptyDOMElement()
  })

  it('says plainly that a system has none, without presenting it as a problem', () => {
    detail('draft')
    const panel = screen.getByRole('heading', { name: 'References' }).closest('section')!
    expect(within(panel).getByText('No references yet.')).toBeInTheDocument()
    expect(within(panel).queryByRole('alert')).not.toBeInTheDocument()
    expect(within(panel).queryByText(/not observed|attention|missing/i)).not.toBeInTheDocument()
  })

  it('shows each resource’s references on its card in Infrastructure', () => {
    renderWith(<RealInfrastructurePage />, inventoryContext({ snapshot }), '/infrastructure')
    const vps = within(screen.getByRole('region', { name: 'VPS' }))
    expect(vps.getByRole('list', { name: 'References' })).toHaveTextContent(
      'Logs/var/log/syslogOn VPSCopy',
    )
    const laptop = within(screen.getByRole('region', { name: 'Laptop' }))
    expect(laptop.getByText('No references yet.')).toBeInTheDocument()
  })

  it('saves the complete list with the application, with no machine for a URL', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot })
    renderWith(<ApplicationForm application={snapshot.applications[0]} onDone={vi.fn()} />, value)
    await user.click(screen.getByRole('button', { name: `Remove local copy ${clone}` }))
    await user.click(screen.getByRole('button', { name: 'Add reference' }))
    const kinds = () => screen.getAllByLabelText('Reference kind')
    const targets = () => screen.getAllByLabelText('URL or path')
    const machines = () => screen.getAllByLabelText('Machine')
    await user.selectOptions(kinds()[1]!, 'Logs')
    await user.type(targets()[1]!, '/var/log/shop')
    expect(machines()[1]).toBeEnabled()
    await user.selectOptions(machines()[1]!, 'VPS · server')
    await user.type(screen.getAllByLabelText('Label')[1]!, 'API')
    // Typed as a path first and then as a URL, its machine goes.
    await user.click(screen.getByRole('button', { name: 'Add reference' }))
    await user.selectOptions(kinds()[2]!, 'Documentation')
    await user.type(targets()[2]!, '/srv/docs')
    await user.selectOptions(machines()[2]!, 'Laptop · workstation')
    await user.clear(targets()[2]!)
    await user.type(targets()[2]!, 'https://docs.example/shop')
    expect(machines()[2]).toBeDisabled()
    await user.type(screen.getAllByLabelText('Label')[2]!, '   ')
    await user.click(screen.getByRole('button', { name: 'Save application' }))
    expect(value.saveApplication).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 'shop',
        references: [
          { id: 'shop-repo', kind: 'repository', target: monorepo, label: null, machineId: null },
          { kind: 'logs', target: '/var/log/shop', label: 'API', machineId: 'vps' },
          {
            kind: 'documentation',
            target: 'https://docs.example/shop',
            label: null,
            machineId: null,
          },
        ],
      }),
    )
  })

  it('lets a resource be the machine of its own paths once it exists', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot })
    renderWith(<ResourceForm resource={snapshot.resources[1]} onDone={vi.fn()} />, value)
    const machine = screen.getByLabelText('Machine')
    expect(machine).toHaveValue('vps')
    expect(within(machine).getByRole('option', { name: 'This resource · server' })).toBeVisible()
    // Every other resource, once.
    expect(
      within(machine)
        .getAllByRole('option')
        .map((option) => option.textContent),
    ).toEqual(['Choose where the path is', 'This resource · server', 'Laptop · workstation'])
    await user.click(screen.getByRole('button', { name: 'Save resource' }))
    expect(value.saveResource).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'vps', references: snapshot.resources[1]!.references }),
    )
  })

  it('offers no resource as its own machine before it exists', async () => {
    const user = userEvent.setup()
    renderWith(<ResourceForm onDone={vi.fn()} />, inventoryContext({ snapshot }))
    await user.click(screen.getByRole('button', { name: 'Add reference' }))
    expect(
      within(screen.getByLabelText('Machine'))
        .getAllByRole('option')
        .map((option) => option.textContent),
    ).toEqual(['Choose where the path is', 'Laptop · workstation', 'VPS · server'])
  })

  it('keeps a resource that holds other systems’ paths, but not for its own', () => {
    expect(deletionBlocker(snapshot, { type: 'resource', id: 'laptop' })).toBe(
      `Holds paths of Shop (local copy ${clone}). Remove those references, or put them on another machine, first.`,
    )
    expect(deletionBlocker(snapshot, { type: 'resource', id: 'vps' })).toBeNull()
  })
})
