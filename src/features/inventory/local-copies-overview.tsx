import { useState, type ReactNode } from 'react'
import { ScanSearch } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { subjectsOf } from '@/domain/evidence'
import { counted } from '@/shared/count'
import { relativeTime } from '@/shared/time'
import { MachineName } from './copy-readings'
import { SubjectLink } from './evidence'
import { inventoryError, type LocalCopy, type ReadOutcome } from './inventory-api'
import { useInventory } from './inventory-context'
import { localCopiesOverview, readAllTargets, unpublishedText } from './local-copies'

/** What a reading held that existed only in its copy, in a few words. */
function heldText(copy: LocalCopy) {
  const reading = copy.latest!
  const commits = (reading.unpublishedOnHead ?? 0) + (reading.unpublishedElsewhere ?? 0)
  return [
    commits
      ? `${unpublishedText(commits)} unpublished ${commits === 1 ? 'commit' : 'commits'}`
      : '',
    reading.stashes ? counted(reading.stashes, 'stash', 'stashes') : '',
    reading.changed ? counted(reading.changed, 'changed file') : '',
    reading.untracked ? counted(reading.untracked, 'new entry', 'new entries') : '',
    reading.unverifiable ? counted(reading.unverifiable, 'unverifiable file') : '',
    reading.operation ? `a ${reading.operation.replace('_', '-')} halfway` : '',
  ]
    .filter(Boolean)
    .join(', ')
}

const notFoundText = {
  missing: 'nothing was at the path',
  not_a_copy: 'not a copy',
  unreadable: 'not readable',
  read: '',
}

function CopyRow({ copy, children }: { copy: LocalCopy; children?: ReactNode }) {
  const { now } = useInventory()
  return (
    <li className="space-y-0.5 py-2.5">
      <p className="flex flex-wrap items-center gap-x-2">
        <SubjectLink subject={copy.subject} className="min-w-0 truncate text-foreground" />
        <span className="text-[11px] text-muted-foreground">
          on <MachineName id={copy.machineId} />
        </span>
      </p>
      <p className="font-mono text-[11px] break-all text-muted-foreground">{copy.path}</p>
      {children && <p className="text-[11px] text-foreground/85">{children}</p>}
      {copy.movedSinceRead && copy.latest && (
        <p className="text-[11px] break-all text-amber-200/90">
          Read at {copy.latest.path}
          {copy.latest.machineId !== copy.machineId ? ' on another machine' : ''}, before it was
          declared here.
        </p>
      )}
      {copy.latest && (
        <p className="text-[11px] text-muted-foreground">
          Read {relativeTime(copy.latest.observedAt, now).toLowerCase()}
          {copy.latest.agent ? `, asked by ${copy.latest.agent}` : ''}
        </p>
      )}
    </li>
  )
}

/**
 * Beside «what needs my attention?» and never in it: what the declared local copies held when they
 * were read, which may no longer be so. Work that existed only in a copy, copies not found where
 * declared, and those of this computer never read, so that an empty list does not reassure more
 * than it should. Copies of archived applications are left out, as from Read all.
 */
export function LocalCopiesOverview() {
  const { snapshot, thisWorkstation, workstationError, readCopies, readingCopies } = useInventory()
  const [outcome, setOutcome] = useState<ReadOutcome | null>(null)
  const [error, setError] = useState<string | null>(null)
  if (!snapshot || !subjectsOf(snapshot).length) return null
  const { current, work, notFound, neverReadHere, neverReadElsewhere } = localCopiesOverview(
    snapshot,
    thisWorkstation,
  )
  const targets = readAllTargets(snapshot, thisWorkstation)
  const running = targets.some((copy) => readingCopies.includes(copy.referenceId))
  const readAll = async () => {
    setOutcome(null)
    setError(null)
    try {
      setOutcome(await readCopies(null))
    } catch (cause) {
      setError(inventoryError(cause))
    }
  }
  return (
    <section aria-labelledby="local-copies-heading">
      <h2 id="local-copies-heading" className="section-heading mb-3">
        Local copies
      </h2>
      <div className="panel space-y-4 p-5 text-xs">
        {!current.length ? (
          <p className="leading-5 text-muted-foreground">
            No system declares a local copy. DevHQ reads only the copies the inventory names: add
            one as a Local copy reference of an application or resource.
          </p>
        ) : (
          <>
            <div>
              <p className="font-medium">
                {thisWorkstation?.resourceId ? (
                  <>
                    {counted(targets.length, 'copy', 'copies')} on this computer,{' '}
                    <MachineName id={thisWorkstation.resourceId} />
                  </>
                ) : (
                  'DevHQ does not know which workstation this computer is'
                )}
              </p>
              <p className="mt-1.5 leading-5 text-muted-foreground">
                {thisWorkstation?.resourceId ? (
                  'DevHQ reads them only when you ask, one after another, without running anything they name. Copies on other computers are read by DevHQ there.'
                ) : (
                  <>
                    {thisWorkstation?.message ?? workstationError ?? 'Asking…'} It reads no copy
                    until{' '}
                    <Link className="text-primary hover:underline" to="/settings">
                      you choose it in Settings
                    </Link>
                    .
                  </>
                )}
              </p>
            </div>
            {targets.length > 0 && (
              <div className="flex flex-wrap items-center gap-3">
                <Button
                  size="sm"
                  variant="outline"
                  disabled={running}
                  onClick={() => void readAll()}
                >
                  <ScanSearch className="size-3.5" aria-hidden="true" />
                  {running ? 'Reading…' : 'Read all on this computer'}
                </Button>
                <p role="status" className="text-[11px] text-muted-foreground empty:hidden">
                  {running
                    ? `Reading ${counted(targets.length, 'copy', 'copies')}, up to 30 s each. You can keep working.`
                    : outcome &&
                      `Read ${counted(outcome.readings.length, 'copy', 'copies')}${outcome.failures.length ? `; ${outcome.failures.length} could not be read and kept nothing` : ''}.`}
                </p>
              </div>
            )}
            {outcome && outcome.failures.length > 0 && (
              <ul role="alert" className="space-y-1 text-[11px] break-words text-rose-300">
                {outcome.failures.map((failure) => (
                  <li key={failure.referenceId}>
                    <SubjectLink subject={failure.subject} />: {failure.message}
                  </li>
                ))}
              </ul>
            )}
            {error && (
              <p role="alert" className="break-words text-rose-300">
                {error}
              </p>
            )}
            <Group title="Work that existed only in a copy" copies={work}>
              {(copy) => heldText(copy)}
            </Group>
            <Group title="Not found where declared" copies={notFound}>
              {(copy) => `When read: ${notFoundText[copy.latest!.outcome]}.`}
            </Group>
            <Group title="Never read on this computer" copies={neverReadHere} />
            {neverReadElsewhere.length > 0 && (
              <p className="leading-5 text-muted-foreground">
                {thisWorkstation?.resourceId
                  ? `${counted(neverReadElsewhere.length, 'copy', 'copies')} on other computers ${neverReadElsewhere.length === 1 ? 'was' : 'were'} never read; DevHQ on each computer reads its own.`
                  : `${counted(neverReadElsewhere.length, 'copy', 'copies')} ${neverReadElsewhere.length === 1 ? 'was' : 'were'} never read.`}
              </p>
            )}
            {!work.length && !notFound.length && !neverReadHere.length && (
              <p className="leading-5 text-muted-foreground">
                No reading found work that existed only in a copy, or a copy missing.
                {neverReadElsewhere.length > 0 && ' Copies never read are not included.'}
              </p>
            )}
          </>
        )}
        <p className="text-[11px] leading-5 text-muted-foreground">
          What each copy held when it was read, which may no longer be so. Not health: none of it is
          in Needs attention.
        </p>
      </div>
    </section>
  )
}

function Group({
  title,
  copies,
  children,
}: {
  title: string
  copies: LocalCopy[]
  children?: (copy: LocalCopy) => string
}) {
  if (!copies.length) return null
  return (
    <div>
      <h3 className="text-[11px] font-medium text-muted-foreground">
        {title} ({copies.length})
      </h3>
      <ul className="mt-1 divide-y border-y">
        {copies.map((copy) => (
          <CopyRow key={copy.referenceId} copy={copy}>
            {children?.(copy)}
          </CopyRow>
        ))}
      </ul>
    </div>
  )
}
