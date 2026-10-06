import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, within } from '@testing-library/react'
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
// Where Windows keeps that folder's files for a DevHQ started from a packaged app.
const packaged =
  'C:\\Users\\Juan Pablo\\AppData\\Local\\Packages\\Claude_pzs8sxrjxfjjc\\LocalCache\\Roaming\\com.devhq.desktop'

function renderSettings(value: InventoryContextValue) {
  return render(
    <MemoryRouter>
      <InventoryContext.Provider value={value}>
        <RealSettingsPage />
      </InventoryContext.Provider>
    </MemoryRouter>,
  )
}

const located = () => vi.fn(async () => ({ database, directory: folder, storedAt: null }))

// Matched on the whole paragraph, which file names in their own elements split.
const paragraph = (text: RegExp) => (_: string, element: Element | null) =>
  element?.tagName === 'P' && text.test(element.textContent ?? '')

describe('the database file in Settings', () => {
  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

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
    expect(screen.getByText('Path copied.').closest('[role="status"]')).not.toBeNull()
    await user.click(screen.getByRole('button', { name: 'Copy path' }))
    expect(
      screen.getByText('The path could not be copied: select it and copy it.'),
    ).toBeInTheDocument()
  })

  it('lets «Path copied.» go after a few seconds and answers every click anew', async () => {
    const writeText = vi
      .fn()
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(new Error('denied'))
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    renderSettings(inventoryContext({ dataLocation: located() }))
    const button = await screen.findByRole('button', { name: 'Copy path' })
    // Only the simulated clock moves from here: real time spent by a busy machine adds nothing.
    // user-event waits on timers of its own, so the click is a plain event inside act, which
    // also lets the copy's promise settle.
    vi.useFakeTimers()
    const click = () => act(async () => void fireEvent.click(button))
    await click()
    const first = screen.getByText('Path copied.')
    act(() => vi.advanceTimersByTime(1000))
    // Copying again shows the confirmation anew and gives it its full time again.
    await click()
    const second = screen.getByText('Path copied.')
    expect(second).not.toBe(first)
    act(() => vi.advanceTimersByTime(2900))
    expect(screen.getByText('Path copied.')).toBe(second)
    act(() => vi.advanceTimersByTime(200))
    expect(screen.queryByText('Path copied.')).not.toBeInTheDocument()
    // A failure says what to do instead, so it stays.
    await click()
    act(() => vi.advanceTimersByTime(10_000))
    expect(
      screen.getByText('The path could not be copied: select it and copy it.'),
    ).toBeInTheDocument()
  })

  it('shows and copies where the file system really keeps it, and says why', async () => {
    const user = userEvent.setup()
    const writeText = vi.fn().mockResolvedValue(undefined)
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
    const kept = `${packaged}\\inventory.sqlite3`
    renderSettings(
      inventoryContext({
        dataLocation: vi.fn(async () => ({ database, directory: folder, storedAt: kept })),
      }),
    )
    // The path File Explorer and every other program open is the one shown and copied.
    expect(await screen.findByText(kept)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Copy path' }))
    expect(writeText).toHaveBeenCalledWith(kept)
    expect(
      screen.getByText(
        paragraph(
          /^DevHQ asked for C:\\Users\\Juan Pablo\\AppData\\Roaming\\com\.devhq\.desktop\\inventory\.sqlite3, but the file system keeps the file at the path above\. .*Claude or Codex desktop app.* DevHQ opened from the Start menu or File Explorer uses the other path, which holds another inventory or none\. Back up, copy and restore at the path above\.$/,
        ),
      ),
    ).toBeInTheDocument()
  })

  it('says nothing more when the file is where DevHQ asked for it', async () => {
    renderSettings(inventoryContext({ dataLocation: located() }))
    expect(await screen.findByText(database)).toBeInTheDocument()
    expect(screen.queryByText(/the file system keeps/)).not.toBeInTheDocument()
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
  storedAt: null,
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
  files: [
    {
      name: backup.name,
      sizeBytes: backup.sizeBytes,
      modifiedAt: takenAt,
      partial: false,
      storedAt: null,
    },
  ],
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
      {
        name: backup.name,
        sizeBytes: 2_097_152,
        modifiedAt: takenAt,
        partial: false,
        storedAt: null,
      },
      {
        name: 'inventory-20260929T080000.000Z.sqlite3.partial',
        sizeBytes: 655_360,
        modifiedAt: '2026-09-29T08:00:01Z',
        partial: true,
        storedAt: null,
      },
      {
        name: 'my-own-copy.sqlite3',
        sizeBytes: 900,
        modifiedAt: null,
        partial: false,
        storedAt: null,
      },
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

  it('says where the file system really keeps a backup, when it is not in the folder', async () => {
    const user = userEvent.setup()
    const kept = `${packaged}\\backups\\${backup.name}`
    const older = 'inventory-20260929T080000.000Z.sqlite3'
    const listBackups = vi.fn().mockResolvedValue({
      ...empty,
      files: [
        { ...withBackup.files[0]!, storedAt: kept },
        { ...withBackup.files[0]!, name: older, storedAt: null },
      ],
    })
    renderSettings(
      inventoryContext({
        dataLocation: located(),
        backupInventory: vi.fn().mockResolvedValue({ ...backup, storedAt: kept }),
        listBackups,
      }),
    )
    await user.click(await screen.findByRole('button', { name: 'Back up now' }))
    const status = screen.getByRole('status', { name: 'Backup' })
    expect(await within(status).findByText(kept)).toBeInTheDocument()
    expect(status).toHaveTextContent(`Kept there by the file system, not at ${backup.path}`)
    // Asked file by file: one is kept elsewhere, the other where it is listed.
    const items = within(
      await screen.findByRole('list', { name: 'Files in the backups folder' }),
    ).getAllByRole('listitem')
    expect(items[0]).toHaveTextContent(`Kept by the file system at ${kept}`)
    expect(items[1]).not.toHaveTextContent('Kept by the file system')
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

  it('drops the list it showed when reading the folder again fails', async () => {
    const user = userEvent.setup()
    const listBackups = vi
      .fn()
      .mockResolvedValueOnce({
        ...empty,
        files: [{ ...withBackup.files[0]!, name: 'older.sqlite3' }],
      })
      .mockRejectedValue({
        code: 'backup_error',
        message: 'DevHQ could not read the backups folder: Access is denied.',
      })
    renderSettings(
      inventoryContext({
        dataLocation: located(),
        backupInventory: vi.fn().mockResolvedValue(backup),
        listBackups,
      }),
    )
    expect(await screen.findByText('older.sqlite3')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Back up now' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'DevHQ could not list the backups folder. DevHQ could not read the backups folder: Access is denied.',
    )
    // The old list lacks the new backup, so it is not shown as the folder's contents.
    expect(screen.queryByText('older.sqlite3')).not.toBeInTheDocument()
    expect(
      screen.queryByRole('list', { name: 'Files in the backups folder' }),
    ).not.toBeInTheDocument()
    expect(listBackups).toHaveBeenCalledTimes(2)
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

describe('the agent gate in Settings', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const gate = () => within(screen.getByRole('region', { name: 'Agent gate' }))

  it('starts off, says so, and gives the command that connects Claude Code once', async () => {
    const user = userEvent.setup()
    const writeText = vi.fn(async () => {})
    vi.stubGlobal('navigator', { clipboard: { writeText } })
    renderSettings(inventoryContext())
    expect(await gate().findByRole('radio', { name: /^Off/ })).toBeChecked()
    expect(gate().getByRole('radio', { name: /^Read only/ })).not.toBeChecked()
    expect(gate().getByText('Never opened: agents cannot read or change anything.')).toBeVisible()
    const command = 'claude mcp add --scope user devhq -- "/opt/devhq/devhq" --mcp'
    expect(gate().getByText(command)).toBeInTheDocument()
    await user.click(gate().getByRole('button', { name: 'Copy command' }))
    expect(writeText).toHaveBeenCalledWith(command)
    expect(gate().getByText('Command copied.')).toBeInTheDocument()
  })

  it('opens the gate as the person chooses and says when it changed', async () => {
    const user = userEvent.setup()
    const value = inventoryContext()
    renderSettings(value)
    await user.click(await gate().findByRole('radio', { name: /^Read only/ }))
    expect(value.setAgentAccess).toHaveBeenCalledWith('read')
    expect(gate().getByRole('radio', { name: /^Read only/ })).toBeChecked()
    expect(await gate().findByText(/^Changed .+ It applies at once/)).toBeInTheDocument()
    // Choosing what is already in force asks for nothing.
    await user.click(gate().getByRole('radio', { name: /^Read only/ }))
    expect(value.setAgentAccess).toHaveBeenCalledTimes(1)
  })

  it('keeps the access in force when changing it fails', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({
      setAgentAccess: vi.fn(async () => {
        throw { code: 'database_error', message: 'Inventory database error: locked' }
      }),
    })
    renderSettings(value)
    await user.click(await gate().findByRole('radio', { name: /^Read and write/ }))
    expect(await gate().findByRole('alert')).toHaveTextContent(
      'The access could not be changed. Inventory database error: locked',
    )
    expect(gate().getByRole('radio', { name: /^Off/ })).toBeChecked()
  })

  it('says when it cannot read the access, instead of showing it off', async () => {
    renderSettings(
      inventoryContext({
        agentAccess: vi.fn(async () => {
          throw { code: 'database_error', message: 'Inventory database error: disk I/O error' }
        }),
      }),
    )
    expect(await gate().findByRole('alert')).toHaveTextContent(
      'DevHQ could not read whether agents may use the gate. Inventory database error: disk I/O error',
    )
    expect(gate().queryByRole('radio')).not.toBeInTheDocument()
  })
})
