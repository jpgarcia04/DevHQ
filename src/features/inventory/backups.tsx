import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { counted } from '@/shared/count'
import { formatBytes } from '@/shared/size'
import { formatMoment } from '@/shared/time'
import { inventoryError, type Backup, type BackupCounts, type BackupFolder } from './inventory-api'
import { useInventory } from './inventory-context'

function contents(counts: BackupCounts) {
  const parts = [
    counted(counts.applications, 'application'),
    counted(counts.components, 'component'),
    counted(counts.resources, 'resource'),
    counted(counts.observations, 'observation'),
    counted(counts.incidents, 'incident'),
    counted(counts.events, 'event'),
    counted(counts.httpChecks, 'HTTP check'),
  ]
  return `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)}`
}

type Attempt =
  | { state: 'idle' }
  | { state: 'running' }
  | { state: 'done'; backup: Backup }
  | { state: 'failed'; error: string }

/**
 * «Back up now» and the backups folder, in Settings › Database file. A backup is checked once,
 * when it is made; the folder is listed as found, without vouching for any file in it.
 */
export function Backups() {
  const { backupInventory, listBackups } = useInventory()
  const [attempt, setAttempt] = useState<Attempt>({ state: 'idle' })
  const [folder, setFolder] = useState<BackupFolder | null>(null)
  const [folderError, setFolderError] = useState<string | null>(null)
  // Read again after every attempt: a backup adds a file, and a failure may leave one.
  const [reads, setReads] = useState(0)

  useEffect(() => {
    let current = true
    listBackups().then(
      (value) => {
        if (!current) return
        setFolder(value)
        setFolderError(null)
      },
      (cause) => {
        if (!current) return
        // A list read before would no longer be what the folder holds: show the failure alone.
        setFolder(null)
        setFolderError(inventoryError(cause))
      },
    )
    return () => {
      current = false
    }
  }, [listBackups, reads])

  const backUp = async () => {
    setAttempt({ state: 'running' })
    try {
      setAttempt({ state: 'done', backup: await backupInventory() })
    } catch (cause) {
      setAttempt({ state: 'failed', error: inventoryError(cause) })
    }
    setReads((count) => count + 1)
  }

  return (
    <div className="mt-6 border-t pt-5">
      <h3 className="text-[13px] font-semibold">Back up now</h3>
      <p className="mt-2 text-xs leading-6 text-muted-foreground">
        Copies the inventory while DevHQ stays open: everything saved until then, nothing
        half-written, without waiting for supervision. DevHQ checks the copy before giving it its
        name, and the timeline records nothing.
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-3">
        <Button size="sm" disabled={attempt.state === 'running'} onClick={() => void backUp()}>
          Back up now
        </Button>
        {attempt.state === 'failed' && (
          <p role="alert" className="text-xs text-rose-300">
            No backup was made. {attempt.error}
          </p>
        )}
      </div>
      <div role="status" aria-label="Backup" className="mt-3 text-xs leading-6">
        {attempt.state === 'running' && <p className="text-muted-foreground">Backing up…</p>}
        {attempt.state === 'done' && <Finished backup={attempt.backup} />}
      </div>
      <p className="mt-4 rounded-md border border-amber-400/20 bg-amber-300/5 px-3 py-2 text-xs leading-6 text-amber-200">
        These backups stay on the same disk as the inventory, inside DevHQ’s own data folder: a
        failing disk takes them along, and uninstalling DevHQ with “Delete the application data”
        ticked deletes the whole folder, backups included. Copy them somewhere else, such as another
        disk or a USB drive, to keep them.
      </p>
      <h3 className="mt-6 text-[13px] font-semibold">Backups folder</h3>
      {folder ? (
        <>
          <p className="mt-3 select-all break-all rounded-md border bg-background/40 px-3 py-2 font-mono text-[11px] text-foreground/90">
            {folder.directory}
          </p>
          <p className="mt-3 text-[11px] leading-5 text-muted-foreground">
            Files as found in the folder. DevHQ checked each of its backups once, when it made it,
            and has not checked any of these files since.
          </p>
          {folder.files.length ? (
            <ul aria-label="Files in the backups folder" className="mt-3 divide-y text-xs">
              {folder.files.map((file) => (
                <li key={file.name} className="flex flex-wrap items-baseline gap-x-4 gap-y-1 py-2">
                  <span className="min-w-0 break-all font-mono text-[11px] text-foreground/90">
                    {file.name}
                  </span>
                  <span className="text-muted-foreground">
                    {file.modifiedAt ? formatMoment(file.modifiedAt) : 'Date unknown'} ·{' '}
                    {formatBytes(file.sizeBytes)}
                  </span>
                  {file.partial && (
                    <span className="text-amber-200">
                      Incomplete: a backup was writing it or was interrupted. It is not a backup.
                    </span>
                  )}
                </li>
              ))}
            </ul>
          ) : (
            <p className="mt-3 text-xs text-muted-foreground">
              No files in the backups folder yet.
            </p>
          )}
        </>
      ) : folderError ? (
        <p role="alert" className="mt-3 text-xs text-rose-300">
          DevHQ could not list the backups folder. {folderError}
        </p>
      ) : (
        <p className="mt-3 text-xs text-muted-foreground">Reading the backups folder…</p>
      )}
    </div>
  )
}

function Finished({ backup }: { backup: Backup }) {
  return (
    <div className="rounded-md border bg-background/40 px-3 py-2">
      <p>
        Backed up at {formatMoment(backup.takenAt)} · {formatBytes(backup.sizeBytes)}
      </p>
      <p className="text-muted-foreground">
        Checked once written: SQLite’s quick check said “{backup.quickCheck}”, schema version{' '}
        {backup.schemaVersion}. It holds {contents(backup.counts)}.
      </p>
      <p className="break-all font-mono text-[11px] text-foreground/90">{backup.path}</p>
    </div>
  )
}
