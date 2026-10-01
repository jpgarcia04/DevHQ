import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type { ReactNode } from 'react'
import type { RecordedEvent } from '@/domain/models'
import { ApplicationForm } from './application-form'
import { deletionBlocker } from './deletion'
import type { InventorySnapshot } from './inventory-api'
import { InventoryContext, type InventoryContextValue } from './inventory-context'
import { RealApplicationDetailPage } from './real-application-detail-page'
import { ResourceForm } from './resource-form'
import { inventoryContext, minutesFromNow } from './test-context'

const note: RecordedEvent = {
  id: 'evt-note',
  subject: { type: 'application', id: 'shop' },
  kind: 'note',
  outcome: 'info',
  title: 'Noted',
  detail: '',
  source: 'manual',
  observationId: null,
  incidentId: null,
  occurredAt: minutesFromNow(-5),
  recordedAt: minutesFromNow(-5),
}

const snapshot: InventorySnapshot = {
  applications: [
    {
      id: 'shop',
      name: 'Shop',
      description: '',
      lifecycle: 'production',
      version: null,
      components: [{ id: 'api', name: 'API', kind: 'backend', resourceId: 'web-host' }],
    },
    {
      id: 'draft',
      name: 'Draft',
      description: '',
      lifecycle: 'development',
      version: null,
      components: [],
    },
  ],
  resources: [
    { id: 'web-host', name: 'Web host', kind: 'server', description: '', operatingSystem: '' },
    { id: 'spare', name: 'Spare box', kind: 'server', description: '', operatingSystem: '' },
  ],
  observations: [],
  incidents: [],
  events: [note],
  httpChecks: [],
  automatedHistory: { limit: 200, truncated: [] },
}

function renderWith(ui: ReactNode, value: InventoryContextValue) {
  return render(
    <MemoryRouter>
      <InventoryContext.Provider value={value}>{ui}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

const app = (id: string) => snapshot.applications.find((item) => item.id === id)!
const resource = (id: string) => snapshot.resources.find((item) => item.id === id)!

describe('deleting applications and resources', () => {
  it('is blocked by history and by components that run on a resource', () => {
    expect(deletionBlocker(snapshot, { type: 'application', id: 'shop' })).toMatch(
      /keeps history.*Archived instead/,
    )
    expect(deletionBlocker(snapshot, { type: 'resource', id: 'web-host' })).toBe(
      'Used by Shop (API). Link those components to another resource, or leave them unlinked, first.',
    )
    expect(deletionBlocker(snapshot, { type: 'application', id: 'draft' })).toBeNull()
    expect(deletionBlocker(snapshot, { type: 'resource', id: 'spare' })).toBeNull()
  })

  it('deletes an application without history after a confirmation', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot })
    const onDeleted = vi.fn()
    renderWith(
      <ApplicationForm application={app('draft')} onDone={vi.fn()} onDeleted={onDeleted} />,
      value,
    )
    await user.click(screen.getByRole('button', { name: 'Delete application…' }))
    // Nothing happens until it is confirmed.
    expect(value.deleteApplication).not.toHaveBeenCalled()
    const confirm = within(screen.getByRole('group', { name: 'Confirm deletion' }))
    expect(confirm.getByText('Delete Draft? This cannot be undone.')).toBeInTheDocument()
    await user.click(confirm.getByRole('button', { name: 'Keep it' }))
    expect(screen.queryByRole('group', { name: 'Confirm deletion' })).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Delete application…' }))
    await user.click(screen.getByRole('button', { name: 'Delete application' }))
    expect(value.deleteApplication).toHaveBeenCalledWith('draft')
    expect(onDeleted).toHaveBeenCalled()
  })

  it('explains why something with history cannot be deleted', () => {
    renderWith(
      <ApplicationForm application={app('shop')} onDone={vi.fn()} />,
      inventoryContext({ snapshot }),
    )
    expect(screen.getByRole('button', { name: 'Delete application…' })).toBeDisabled()
    expect(screen.getByText(/DevHQ keeps history, so it cannot be deleted/)).toBeInTheDocument()
  })

  it('offers no deletion while creating', () => {
    renderWith(<ApplicationForm onDone={vi.fn()} />, inventoryContext({ snapshot }))
    expect(screen.queryByRole('button', { name: /Delete/ })).not.toBeInTheDocument()
  })

  it('shows the native refusal when the rules changed meanwhile, and stays', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({
      snapshot,
      deleteResource: vi.fn(async () => {
        throw {
          code: 'invalid_input',
          message:
            'Spare box has 1 observation and 1 event. DevHQ keeps history, so it cannot be deleted.',
        }
      }),
    })
    const onDone = vi.fn()
    renderWith(<ResourceForm resource={resource('spare')} onDone={onDone} />, value)
    expect(screen.getByRole('button', { name: 'Delete resource…' })).toBeEnabled()
    await user.click(screen.getByRole('button', { name: 'Delete resource…' }))
    await user.click(screen.getByRole('button', { name: 'Delete resource' }))
    expect(value.deleteResource).toHaveBeenCalledWith('spare')
    expect(screen.getByRole('alert')).toHaveTextContent('Spare box has 1 observation and 1 event.')
    expect(onDone).not.toHaveBeenCalled()
  })

  it('keeps a resource that components run on', () => {
    renderWith(
      <ResourceForm resource={resource('web-host')} onDone={vi.fn()} />,
      inventoryContext({ snapshot }),
    )
    expect(screen.getByRole('button', { name: 'Delete resource…' })).toBeDisabled()
    expect(screen.getByText(/^Used by Shop \(API\)/)).toBeInTheDocument()
  })

  it('returns to the applications list once the open application is deleted', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot })
    render(
      <MemoryRouter initialEntries={['/applications/draft']}>
        <InventoryContext.Provider value={value}>
          <Routes>
            <Route path="/applications" element={<h1>All applications</h1>} />
            <Route path="/applications/:applicationId" element={<RealApplicationDetailPage />} />
          </Routes>
        </InventoryContext.Provider>
      </MemoryRouter>,
    )
    await user.click(screen.getByRole('button', { name: 'Edit application' }))
    await user.click(screen.getByRole('button', { name: 'Delete application…' }))
    await user.click(screen.getByRole('button', { name: 'Delete application' }))
    expect(value.deleteApplication).toHaveBeenCalledWith('draft')
    expect(screen.getByRole('heading', { name: 'All applications' })).toBeInTheDocument()
  })
})
