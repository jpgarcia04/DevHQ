import { CircleDashed } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { counted } from '@/shared/count'
import { formatBytes } from '@/shared/size'
import { formatMoment } from '@/shared/time'
import {
  inventoryError,
  type BeforeUpgrade,
  type TableRows,
  type UpgradeInspection,
  type UpgradeStep,
  type Upgraded,
} from './inventory-api'
import { useInventory } from './inventory-context'

type Look =
  | { state: 'looking' }
  | { state: 'seen'; inspection: UpgradeInspection }
  | { state: 'unseen'; error: string }

type Attempt =
  | { state: 'idle' }
  | { state: 'running'; steps: UpgradeStep[] }
  | { state: 'failed'; error: string; backup: BeforeUpgrade | null }

/** The steps of an upgrade in the order the native process reaches them, as the person reads them. */
const stepNames: Record<UpgradeStep['step'], string> = {
  checking: 'Checking the inventory again and taking the write lock',
  backingUp: 'Backing up and verifying the copy',
  backedUp: 'Backup verified and named, with its manifest',
  upgrading: 'Upgrading, in a transaction nothing else sees yet',
  verifying: 'Verifying the upgraded inventory before committing',
}

function holds(tables: TableRows[]) {
  const rows = tables.reduce((sum, table) => sum + table.rows, 0)
  return `${counted(rows, 'row')} in ${counted(tables.length, 'table')}`
}

/**
 * The window's answer to an inventory in an older schema: what it is, read without changing it,
 * and, when DevHQ can vouch for it, Back up and upgrade, which the person starts and nothing else
 * does. There is no way to upgrade without the backup. Every state it shows is one the native
 * process reported.
 */
export function InventoryUpgrade({ refusal }: { refusal: string | null }) {
  const { inspectUpgrade, upgradeInventory } = useInventory()
  const [look, setLook] = useState<Look>({ state: 'looking' })
  const [attempt, setAttempt] = useState<Attempt>({ state: 'idle' })
  // Looked at again after a failed attempt: what is there may have changed.
  const [looks, setLooks] = useState(0)

  useEffect(() => {
    let current = true
    inspectUpgrade().then(
      (inspection) => current && setLook({ state: 'seen', inspection }),
      (cause) => current && setLook({ state: 'unseen', error: inventoryError(cause) }),
    )
    return () => {
      current = false
    }
  }, [inspectUpgrade, looks])

  const upgrade = async (inspection: UpgradeInspection) => {
    const steps: UpgradeStep[] = []
    setAttempt({ state: 'running', steps })
    try {
      await upgradeInventory(
        inspection.storedVersion ?? 0,
        inspection.fingerprint ?? '',
        (step) => {
          steps.push(step)
          setAttempt({ state: 'running', steps: [...steps] })
        },
      )
      // The provider shows the outcome above the inventory it now serves.
    } catch (cause) {
      const made = steps.find((step) => step.step === 'backedUp')
      setAttempt({
        state: 'failed',
        error: inventoryError(cause),
        backup: made?.step === 'backedUp' ? made.backup : null,
      })
      setLook({ state: 'looking' })
      setLooks((count) => count + 1)
    }
  }

  return (
    <section
      aria-labelledby="inventory-upgrade-heading"
      className="rounded-xl border border-dashed px-6 py-10"
    >
      <div className="flex flex-col items-center text-center">
        <CircleDashed className="mb-4 size-7 text-muted-foreground" aria-hidden="true" />
        <h2 id="inventory-upgrade-heading" className="font-medium">
          {look.state === 'seen' && look.inspection.state === 'current'
            ? 'Inventory already upgraded'
            : 'Inventory needs a schema upgrade'}
        </h2>
      </div>
      <div className="mx-auto mt-4 max-w-2xl text-sm leading-relaxed">
        {attempt.state === 'running' ? (
          <Progress steps={attempt.steps} />
        ) : (
          <>
            {attempt.state === 'failed' && (
              <div role="alert" className="mb-5 rounded-md border border-rose-400/30 px-3 py-2">
                <p className="font-medium text-rose-300">The upgrade did not happen.</p>
                <p className="mt-1 text-xs leading-6 text-rose-200">{attempt.error}</p>
                {attempt.backup && <BackupMade backup={attempt.backup} />}
              </div>
            )}
            {look.state === 'looking' && (
              <p role="status" className="text-center text-muted-foreground">
                Looking at the inventory without changing it…
              </p>
            )}
            {look.state === 'unseen' && (
              <div role="alert" className="text-center">
                <p className="text-muted-foreground">{refusal}</p>
                <p className="mt-2 text-rose-300">DevHQ could not look at it: {look.error}</p>
                <Button
                  className="mt-4"
                  variant="outline"
                  onClick={() => {
                    setLook({ state: 'looking' })
                    setLooks((count) => count + 1)
                  }}
                >
                  Look again
                </Button>
              </div>
            )}
            {look.state === 'seen' && (
              <Inspected
                inspection={look.inspection}
                onUpgrade={() => void upgrade(look.inspection)}
              />
            )}
          </>
        )}
      </div>
    </section>
  )
}

function Inspected({
  inspection,
  onUpgrade,
}: {
  inspection: UpgradeInspection
  onUpgrade: () => void
}) {
  const file = inspection.storedAt ?? inspection.database
  if (inspection.state === 'current')
    return (
      <div className="text-center">
        <p className="text-muted-foreground">
          {file} is already in schema version {inspection.version}: another DevHQ upgraded it after
          this window started. Nothing needs to be written.
        </p>
        <Button className="mt-4" onClick={onUpgrade}>
          Open it
        </Button>
      </div>
    )
  if (inspection.state !== 'outdated' || !inspection.upgradable)
    return (
      <div>
        <p className="text-muted-foreground">
          <span className="break-all font-mono text-xs text-foreground/90">{file}</span>:{' '}
          {inspection.message}
        </p>
        <p className="mt-3 font-medium">
          DevHQ does not upgrade it: it never changes what it cannot vouch for.
        </p>
        <ul className="mt-2 list-disc space-y-1 pl-5 text-xs leading-6 text-amber-200">
          {inspection.problems.map((problem) => (
            <li key={problem}>{problem}</li>
          ))}
        </ul>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          Nothing was changed. Settings shows where the file and its backups folder are.
        </p>
      </div>
    )
  const from = inspection.storedVersion
  return (
    <div>
      <p className="text-muted-foreground">
        <span className="break-all font-mono text-xs text-foreground/90">{file}</span> is a DevHQ
        inventory in schema version {from}; this DevHQ reads version {inspection.version}. DevHQ
        read it without changing it, and serves nothing from it until it is upgraded.
      </p>
      <p className="mt-3">
        It holds {holds(inspection.tables)}. SQLite’s integrity check, every relationship and the
        structure of schema version {from} were checked: DevHQ can upgrade it.
      </p>
      <ul aria-label="What the inventory holds" className="mt-2 flex flex-wrap gap-x-4 text-xs">
        {inspection.tables.map((table) => (
          <li key={table.table} className="text-muted-foreground">
            <span className="font-mono">{table.table}</span> {table.rows.toLocaleString('en-GB')}
          </li>
        ))}
      </ul>
      <h3 className="mt-5 text-[13px] font-semibold">Back up and upgrade</h3>
      <ol className="mt-2 list-decimal space-y-1 pl-5 text-xs leading-6 text-muted-foreground">
        <li>
          Copies the inventory into{' '}
          <span className="break-all font-mono text-foreground/90">{inspection.backups}</span>,
          named for schema version {from}, with a manifest beside it, and checks every row of the
          copy against the inventory before naming it. There is no upgrade without this backup.
        </li>
        <li>
          Upgrades the inventory in one transaction, committed only once the result passes SQLite’s
          integrity check and still holds every row it held, value by value.
        </li>
        <li>
          If anything fails, or DevHQ closes before the commit, nothing of the upgrade is kept and
          the backup stays. Once upgraded, a DevHQ older than this one cannot open the inventory:
          going back means putting the backup in its place.
        </li>
      </ol>
      <p className="mt-3 rounded-md border border-amber-400/20 bg-amber-300/5 px-3 py-2 text-xs leading-6 text-amber-200">
        The backup stays on the same disk as the inventory: copy it somewhere else to keep it.
      </p>
      <div className="mt-5 flex justify-center">
        <Button onClick={onUpgrade}>Back up and upgrade</Button>
      </div>
    </div>
  )
}

function Progress({ steps }: { steps: UpgradeStep[] }) {
  const reached = new Set(steps.map((step) => step.step))
  const backup = steps.find((step) => step.step === 'backedUp')
  const order = Object.keys(stepNames) as UpgradeStep['step'][]
  const last = steps.at(-1)?.step
  return (
    <div role="status" aria-label="Upgrade">
      <ol className="space-y-1 text-xs leading-6">
        {order.map((step) => (
          <li
            key={step}
            className={reached.has(step) ? 'text-foreground' : 'text-muted-foreground/60'}
          >
            {reached.has(step) ? (step === last && step !== 'backedUp' ? '… ' : '✓ ') : '· '}
            {stepNames[step]}
          </li>
        ))}
      </ol>
      {backup?.step === 'backedUp' && <BackupMade backup={backup.backup} />}
    </div>
  )
}

function BackupMade({ backup }: { backup: BeforeUpgrade }) {
  return (
    <div className="mt-3 rounded-md border bg-background/40 px-3 py-2 text-xs leading-6">
      <p>
        Backup of schema version {backup.schemaVersion}, taken at {formatMoment(backup.takenAt)} ·{' '}
        {formatBytes(backup.sizeBytes)} · integrity check “{backup.integrityCheck}” ·{' '}
        {holds(backup.tables)}, each checked against the inventory
      </p>
      <p className="break-all font-mono text-[11px] text-foreground/90">
        {backup.storedAt ?? backup.path}
      </p>
      {backup.storedAt && (
        <p className="text-[11px] text-amber-200">
          Kept there by the file system, not at{' '}
          <span className="break-all font-mono">{backup.path}</span>
        </p>
      )}
      <p className="text-muted-foreground">
        Manifest: <span className="break-all font-mono text-[11px]">{backup.manifest}</span>
      </p>
      <p className="text-muted-foreground">
        SHA-256: <span className="break-all font-mono text-[11px]">{backup.sha256}</span>
      </p>
    </div>
  )
}

/** What the person’s upgrade did, above the inventory the window now serves, until dismissed. */
export function UpgradeDone({
  upgraded,
  onDismiss,
}: {
  upgraded: Upgraded
  onDismiss: () => void
}) {
  return (
    <section
      aria-labelledby="inventory-upgraded-heading"
      className="mb-6 rounded-xl border border-emerald-400/25 bg-emerald-300/5 px-5 py-4 text-sm"
    >
      <h2 id="inventory-upgraded-heading" className="font-medium">
        {upgraded.backup ? 'Inventory upgraded' : 'Inventory already upgraded'}
      </h2>
      {upgraded.backup ? (
        <>
          <p className="mt-1 text-muted-foreground">
            From schema version {upgraded.from} to {upgraded.to}. All{' '}
            {counted(upgraded.rowsKept, 'row')} in {counted(upgraded.tablesKept, 'table')} were
            carried over and checked value by value, with SQLite’s integrity check and every
            relationship, before the upgrade was committed.
          </p>
          <BackupMade backup={upgraded.backup} />
          <p className="mt-3 text-xs leading-6 text-amber-200">
            Copy the backup and its manifest somewhere else, such as another disk: they are on the
            same disk as the inventory.
          </p>
        </>
      ) : (
        <p className="mt-1 text-muted-foreground">
          Another DevHQ upgraded it to schema version {upgraded.to} meanwhile. Nothing was written,
          and no second backup was made.
        </p>
      )}
      <Button className="mt-3" size="sm" variant="outline" onClick={onDismiss}>
        Dismiss
      </Button>
    </section>
  )
}
