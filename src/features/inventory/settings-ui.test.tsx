import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { formatBytes } from '@/shared/size'
import { formatMoment } from '@/shared/time'
import type { Backup, BackupFolder } from './inventory-api'
import { InventoryContext, type InventoryContextValue } from './inventory-context'
import { RealSettingsPage } from './real-settings-page'
import { inventoryContext } from './test-context'

const folder = 'C:\\Users\\Juan Pablo\\AppData\\Roaming\\com.devhq.desktop'
const database = `${folder}\\inventory.sqlite3`

function renderSettings(value: InventoryContextValue) {
  return render(
    <MemoryRouter>
      <InventoryContext.Provider value={value}>
        <RealSettingsPage />
      </InventoryContext.Provider>
    </MemoryRouter>,
  )
}

const located = () => vi.fn(async () => ({ database, directory: folder }))

// Matched on the whole paragraph, which file names in their own elements split.
const paragraph = (text: RegExp) => (_: string, element: Element | null) =>
  element?.tagName === 'P' && text.test(element.textContent ?? '')

describe('the database file in Settings', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('shows where the native process keeps it and how to back it up', async () => {
    renderSettings(inventoryContext({ dataLocation: located() }))
    expect(screen.getByText('Locating the database file…')).toBeInTheDocument()
    expect(await screen.findByText(database)).toBeInTheDocument()
    expect(
      screen.getByText(
        paragraph(/^With DevHQ closed, a copy of the database file is also a complete backup\./),
      ),
    ).toBeInTheDocument()
    expect(
      screen.getByText(
        paragraph(
          /under the name inventory\.sqlite3 and delete inventory\.sqlite3-wal and inventory\.sqlite3-shm/,
        ),
      ),
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
    expect(screen.getByText('Path copied.')).toHaveAttribute('role', 'status')
    await user.click(screen.getByRole('button', { name: 'Copy path' }))
    expect(
      screen.getByText('The path could not be copied: select it and copy it.'),
    ).toBeInTheDocument()
  })

  it('answers even when the inventory cannot be read', async () => {
    renderSettings(
      inventoryContext({
        dataLocation: located(),
        listBackups: vi.fn().mockResolvedValue(empty),
        snapshot: null,
        loadState: 'error',
        error: 'Inventory database error: file is not a database',
      }),
    )
    expect(await screen.findByText(database)).toBeInTheDocument()
    expect(await screen.findByText(`${folder}\\backups`)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Back up now' })).toBeEnabled()
  })

  it('reports a failed request instead of a path', async () => {
    renderSettings(
      inventoryContext({
        dataLocation: vi.fn().mockRejectedValue({ code: 'ipc', message: 'IPC unavailable' }),
      }),
    )
    expect(
      await screen.findByText(/^DevHQ could not say where its database file is\./),
    ).toHaveTextContent('DevHQ could not say where its database file is. IPC unavailable')
    expect(screen.queryByRole('button', { name: 'Copy path' })).not.toBeInTheDocument()
  })
})

const takenAt = '2026-09-30T11:58:12.345Z'
const backup: Backup = {
  name: 'inventory-20260930T115812.345Z.sqlite3',
  path: `${folder}\\backups\\inventory-20260930T115812.345Z.sqlite3`,
  takenAt,
  sizeBytes: 2_097_152,
  quickCheck: 'ok',
  schemaVersion: 6,
  counts: {
    applications: 1,
    components: 2,
    resources: 1,
    observations: 1_204,
    incidents: 1,
    events: 1_310,
    httpChecks: 1,
  },
}
const empty: BackupFolder = { directory: `${folder}\\backups`, files: [] }
const withBackup: BackupFolder = {
  ...empty,
  files: [{ name: backup.name, sizeBytes: backup.sizeBytes, modifiedAt: takenAt, partial: false }],
}

/** A promise settled by hand, to look at the page while a backup is being made. */
function deferred<T>() {
  let settle!: { resolve: (value: T) => void; reject: (cause: unknown) => void }
  const promise = new Promise<T>((resolve, reject) => {
    settle = { resolve, reject }
  })
  return { promise, ...settle }
}

describe('Back up now', () => {
  it('says it is backing up, then when, how big, what it checked and what the copy holds', async () => {
    const user = userEvent.setup()
    const pending = deferred<Backup>()
    const backupInventory = vi.fn(() => pending.promise)
    const listBackups = vi.fn().mockResolvedValueOnce(empty).mockResolvedValue(withBackup)
    renderSettings(inventoryContext({ dataLocation: located(), backupInventory, listBackups }))
    expect(await screen.findByText('No files in the backups folder yet.')).toBeInTheDocument()

    const button = screen.getByRole('button', { name: 'Back up now' })
    await user.click(button)
    const status = screen.getByRole('status', { name: 'Backup' })
    expect(status).toHaveTextContent('Backing up…')
    // One at a time: the button waits for the backup in progress.
    expect(button).toBeDisabled()
    await user.click(button)
    expect(backupInventory).toHaveBeenCalledTimes(1)

    pending.resolve(backup)
    expect(
      await within(status).findByText(`Backed up at ${formatMoment(takenAt)} · 2 MB`),
    ).toBeInTheDocument()
    expect(status).toHaveTextContent(
      'Checked once written: SQLite’s quick check said “ok”, schema version 6. It holds 1 application, 2 components, 1 resource, 1,204 observations, 1 incident, 1,310 events and 1 HTTP check.',
    )
    expect(status).toHaveTextContent(backup.path)
    expect(button).toBeEnabled()
    // The folder is read again and now lists it.
    const files = await screen.findByRole('list', { name: 'Files in the backups folder' })
    expect(within(files).getByText(backup.name)).toBeInTheDocument()
    expect(listBackups).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('reports a failed backup, claims nothing and lets you try again', async () => {
    const user = userEvent.setup()
    const backupInventory = vi
      .fn()
      .mockRejectedValueOnce({
        code: 'backup_error',
        message:
          'Copying the inventory failed: database or disk is full. The incomplete copy was removed, so no backup was added.',
      })
      .mockResolvedValueOnce(backup)
    const listBackups = vi.fn().mockResolvedValue(empty)
    renderSettings(inventoryContext({ dataLocation: located(), backupInventory, listBackups }))
    await user.click(screen.getByRole('button', { name: 'Back up now' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'No backup was made. Copying the inventory failed: database or disk is full. The incomplete copy was removed, so no backup was added.',
    )
    expect(screen.getByRole('status', { name: 'Backup' })).toBeEmptyDOMElement()
    expect(listBackups).toHaveBeenCalledTimes(2)

    await user.click(screen.getByRole('button', { name: 'Back up now' }))
    expect(
      await screen.findByText(`Backed up at ${formatMoment(takenAt)} · 2 MB`),
    ).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('lists the backups folder as found, without vouching for any file in it', async () => {
    const files: BackupFolder['files'] = [
      { name: backup.name, sizeBytes: 2_097_152, modifiedAt: takenAt, partial: false },
      {
        name: 'inventory-20260929T080000.000Z.sqlite3.partial',
        sizeBytes: 655_360,
        modifiedAt: '2026-09-29T08:00:01Z',
        partial: true,
      },
      { name: 'my-own-copy.sqlite3', sizeBytes: 900, modifiedAt: null, partial: false },
    ]
    renderSettings(
      inventoryContext({
        dataLocation: located(),
        listBackups: vi.fn().mockResolvedValue({ ...empty, files }),
      }),
    )
    const list = await screen.findByRole('list', { name: 'Files in the backups folder' })
    const items = within(list).getAllByRole('listitem')
    expect(items.map((item) => item.textContent)).toEqual([
      `${backup.name}${formatMoment(takenAt)} · ${formatBytes(2_097_152)}`,
      `inventory-20260929T080000.000Z.sqlite3.partial${formatMoment('2026-09-29T08:00:01Z')} · 640 KB` +
        'Incomplete: a backup was writing it or was interrupted. It is not a backup.',
      'my-own-copy.sqlite3Date unknown · 900 B',
    ])
    // Checked once, when made: nothing in the folder is presented as sound.
    expect(list).not.toHaveTextContent(/checked|verified|healthy|ok\b/i)
    expect(
      screen.getByText(
        'Files as found in the folder. DevHQ checked each of its backups once, when it made it, and has not checked any of these files since.',
      ),
    ).toBeInTheDocument()
  })

  it('says when the folder cannot be read', async () => {
    renderSettings(
      inventoryContext({
        dataLocation: located(),
        listBackups: vi.fn().mockRejectedValue({
          code: 'backup_error',
          message: 'DevHQ could not read the backups folder: Access is denied.',
        }),
      }),
    )
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'DevHQ could not list the backups folder. DevHQ could not read the backups folder: Access is denied.',
    )
    expect(screen.getByRole('button', { name: 'Back up now' })).toBeEnabled()
  })

  it('warns that backups share the disk and the folder the uninstaller deletes on request', async () => {
    renderSettings(inventoryContext({ dataLocation: located() }))
    expect(
      screen.getByText(
        'These backups stay on the same disk as the inventory, inside DevHQ’s own data folder: a failing disk takes them along, and uninstalling DevHQ with “Delete the application data” ticked deletes the whole folder, backups included. Copy them somewhere else, such as another disk or a USB drive, to keep them.',
      ),
    ).toBeInTheDocument()
    await screen.findByText(database)
  })
})
