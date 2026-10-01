import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { InventoryContext, type InventoryContextValue } from './inventory-context'
import { RealSettingsPage } from './real-pages'
import { inventoryContext } from './test-context'

const database = 'C:\\Users\\jp\\AppData\\Roaming\\com.devhq.desktop\\inventory.sqlite3'

function renderSettings(value: InventoryContextValue) {
  return render(
    <MemoryRouter>
      <InventoryContext.Provider value={value}>
        <RealSettingsPage />
      </InventoryContext.Provider>
    </MemoryRouter>,
  )
}

const located = () =>
  vi.fn(async () => ({ database, directory: database.replace('\\inventory.sqlite3', '') }))

describe('the database file in Settings', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('shows where the native process keeps it and how to back it up', async () => {
    renderSettings(inventoryContext({ dataLocation: located() }))
    expect(screen.getByRole('status')).toHaveTextContent('Locating the database file…')
    expect(await screen.findByText(database)).toBeInTheDocument()
    // Matched on the whole paragraph, which file names in their own elements split.
    const paragraph = (text: RegExp) => (_: string, element: Element | null) =>
      element?.tagName === 'P' && text.test(element.textContent ?? '')
    expect(
      screen.getByText(paragraph(/^To back it up, close DevHQ and copy this file\./)),
    ).toBeInTheDocument()
    expect(
      screen.getByText(paragraph(/delete inventory\.sqlite3-wal and inventory\.sqlite3-shm/)),
    ).toBeInTheDocument()
  })

  it('copies the path, or says to copy it by hand', async () => {
    const user = userEvent.setup()
    const writeText = vi
      .fn()
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(new Error('denied'))
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    renderSettings(inventoryContext({ dataLocation: located() }))
    await user.click(await screen.findByRole('button', { name: 'Copy path' }))
    expect(writeText).toHaveBeenCalledWith(database)
    expect(screen.getByRole('status')).toHaveTextContent('Path copied.')
    await user.click(screen.getByRole('button', { name: 'Copy path' }))
    expect(screen.getByRole('status')).toHaveTextContent(
      'The path could not be copied: select it and copy it.',
    )
  })

  it('answers even when the inventory cannot be read', async () => {
    renderSettings(
      inventoryContext({
        dataLocation: located(),
        snapshot: null,
        loadState: 'error',
        error: 'Inventory database error: file is not a database',
      }),
    )
    expect(await screen.findByText(database)).toBeInTheDocument()
  })

  it('reports a failed request instead of a path', async () => {
    renderSettings(
      inventoryContext({
        dataLocation: vi.fn().mockRejectedValue({ code: 'ipc', message: 'IPC unavailable' }),
      }),
    )
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'DevHQ could not say where its database file is. IPC unavailable',
    )
    expect(screen.queryByRole('button', { name: 'Copy path' })).not.toBeInTheDocument()
  })
})
