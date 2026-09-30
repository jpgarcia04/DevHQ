import { describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import type { ReactNode } from 'react'
import { InventoryContext, useInventory, type InventoryContextValue } from './inventory-context'
import { ApplicationForm } from './application-form'
import { RealInfrastructurePage, RealOverviewPage } from './real-pages'
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
    expect(main.queryByText('Healthy')).not.toBeInTheDocument()
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
})
