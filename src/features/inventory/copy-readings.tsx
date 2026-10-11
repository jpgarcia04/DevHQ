import { useEffect, useId, useState } from 'react'
import { ChevronDown, FolderGit2, ScanSearch } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import type { EntityRef } from '@/domain/models'
import { counted } from '@/shared/count'
import { formatDateTime, relativeTime } from '@/shared/time'
import { cn } from '@/shared/utils'
import { SubjectLink } from './evidence'
import {
  inventoryError,
  type CopyOperation,
  type CopyReading,
  type LocalCopy,
  type ReadingOutcome,
} from './inventory-api'
import { useInventory } from './inventory-context'
import { onThisComputer, unpublishedText } from './local-copies'

/** Readings a page holds, as the native process pages them (`READINGS_PAGE`). */
const readingsPage = 200

const outcomeLabels: Record<ReadingOutcome, string> = {
  read: 'Read',
  missing: 'Not found',
  not_a_copy: 'Not a copy',
  unreadable: 'Not readable',
}

const operationLabels: Record<CopyOperation, string> = {
  merge: 'A merge',
  rebase: 'A rebase',
  cherry_pick: 'A cherry-pick',
  revert: 'A revert',
  bisect: 'A bisect',
  apply_mailbox: 'An am (applying patches)',
}

function Moment({ value, now }: { value: string; now: number }) {
  // A commit can carry any date, a later one than the reading included: then no "ago".
  const later = Date.parse(value) > now
  return (
    <>
      <time dateTime={value}>{formatDateTime(value)}</time>
      {later ? ' (a date still to come)' : ` (${relativeTime(value, now).toLowerCase()})`}
    </>
  )
}

/** A machine a reading or a copy names, which may have left the inventory since. */
export function MachineName({ id }: { id: string }) {
  const { snapshot } = useInventory()
  if (!snapshot?.resources.some((resource) => resource.id === id))
    return <span className="text-muted-foreground">a machine no longer in the inventory</span>
  // Said inside sentences, so it is underlined: colour alone would not set it apart.
  return (
    <SubjectLink
      subject={{ type: 'resource', id }}
      className="underline decoration-dotted underline-offset-2"
    />
  )
}

/**
 * A reading's provenance: DevHQ read it itself, with the library named in the tooltip, and an
 * agent's name when one asked for it through the gate. No health colour: a reading is not health.
 */
export function ReadingTag({ reading }: { reading: CopyReading }) {
  return (
    <span
      className="inline-flex max-w-full min-w-0 shrink-0 items-center gap-1 rounded border border-indigo-300/30 bg-indigo-300/5 px-1.5 py-0.5 text-[10px] text-indigo-200"
      title={
        reading.agent
          ? `Read with ${reading.reader}, when an agent asked through the agent gate. The name is the one its client declared; DevHQ cannot verify it.`
          : `Read with ${reading.reader}, when you asked.`
      }
    >
      <ScanSearch className="size-2.5 shrink-0" aria-hidden="true" />
      <span className="truncate">
        Automated · Local read{reading.agent ? ` · asked by ${reading.agent}` : ''}
      </span>
    </span>
  )
}

/** When and where a reading was made, and who asked: always shown with what it found. */
function ReadingHeadline({ reading, now }: { reading: CopyReading; now: number }) {
  return (
    <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
      <span
        className={cn(
          'rounded border px-1.5 py-0.5 text-[10px]',
          reading.outcome === 'read' ? 'text-foreground' : 'border-dashed text-amber-200/90',
        )}
      >
        {outcomeLabels[reading.outcome]}
      </span>
      <span>
        <Moment value={reading.observedAt} now={now} /> on <MachineName id={reading.machineId} />
      </span>
      <ReadingTag reading={reading} />
    </p>
  )
}

function workText(reading: CopyReading) {
  const parts: string[] = []
  if (reading.unpublishedOnHead)
    parts.push(
      `${unpublishedText(reading.unpublishedOnHead)} ${reading.unpublishedOnHead === 1 ? 'commit' : 'commits'} of HEAD`,
    )
  if (reading.unpublishedElsewhere)
    parts.push(
      `${unpublishedText(reading.unpublishedElsewhere)} ${reading.unpublishedElsewhere === 1 ? 'commit' : 'commits'} of other local branches`,
    )
  return parts.length ? `${parts.join(' and ')} on no remote branch` : 'None'
}

function worktreeText(reading: CopyReading) {
  if (reading.worktreeUnknown !== null) return `Unknown: ${reading.worktreeUnknown}`
  const parts = [
    reading.changed ? counted(reading.changed, 'changed file') : null,
    reading.untracked ? counted(reading.untracked, 'new entry', 'new entries') : null,
    reading.unverifiable
      ? `${counted(reading.unverifiable, 'file')} DevHQ could not verify (a filter it does not run)`
      : null,
  ].filter((part) => part !== null)
  return parts.length ? parts.join(' · ') : 'No changes and no new entries'
}

/** What a reading found, in one line, for a list of many: the history of a system. */
function readingSummary(reading: CopyReading) {
  if (reading.outcome !== 'read') return reading.detail ?? ''
  const held = [
    workText(reading) === 'None' ? null : workText(reading),
    reading.stashes ? counted(reading.stashes, 'stash', 'stashes') : null,
    reading.operation ? `${operationLabels[reading.operation]} in progress` : null,
    worktreeText(reading),
  ].filter((part) => part !== null)
  const head = reading.head
    ? `${reading.branch ?? 'Detached'} at ${reading.head.slice(0, 7)}`
    : `${reading.branch ?? 'Detached'}, no commit yet`
  const last = reading.lastCommitAt ? `last commit ${formatDateTime(reading.lastCommitAt)}` : null
  return [head, last, ...held].filter(Boolean).join(' · ')
}

/**
 * What a reading found, in the past: what the copy held when it was read, which may no longer be
 * so. Never author, message or file names: the reading keeps none.
 */
export function ReadingFacts({ reading, now }: { reading: CopyReading; now: number }) {
  if (reading.outcome !== 'read')
    return (
      <p className="mt-1.5 text-[11px] leading-5 break-words text-muted-foreground">
        {reading.detail}
      </p>
    )
  const head = reading.head?.slice(0, 7)
  return (
    <dl
      aria-label="What the reading found"
      className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 rounded-md border border-indigo-300/10 bg-indigo-300/[0.03] px-3 py-2 text-[11px] text-muted-foreground"
    >
      <dt>HEAD</dt>
      <dd className="break-words text-foreground/85">
        {reading.branch === null ? 'Detached' : reading.branch}
        {head ? (
          <>
            {' '}
            at <span className="font-mono">{head}</span>
          </>
        ) : (
          ', no commit yet'
        )}
      </dd>
      <dt>Last commit</dt>
      <dd>{reading.lastCommitAt ? <Moment value={reading.lastCommitAt} now={now} /> : 'None'}</dd>
      <dt>Unpublished</dt>
      <dd className={cn(workText(reading) !== 'None' && 'text-foreground/85')}>
        {workText(reading)}
      </dd>
      <dt>Stashes</dt>
      <dd className={cn(!!reading.stashes && 'text-foreground/85')}>{reading.stashes || 'None'}</dd>
      <dt>Working tree</dt>
      <dd className="break-words">{worktreeText(reading)}</dd>
      {reading.operation && (
        <>
          <dt>Operation</dt>
          <dd className="text-foreground/85">
            {operationLabels[reading.operation]} was in progress
          </dd>
        </>
      )}
      <dt>Last sync</dt>
      <dd>
        {reading.syncedAt ? (
          <Moment value={reading.syncedAt} now={now} />
        ) : (
          'No sign of one in its files'
        )}
      </dd>
      <dt>Remotes</dt>
      <dd>
        {reading.remotes?.length ? (
          <ul className="space-y-0.5">
            {reading.remotes.map((remote) => (
              <li key={remote.name} className="break-all">
                {remote.name}
                {remote.url ? (
                  <span className="font-mono text-foreground/85"> {remote.url}</span>
                ) : (
                  <span> · URL not shown: not a form DevHQ keeps without credentials</span>
                )}
              </li>
            ))}
          </ul>
        ) : (
          'None'
        )}
      </dd>
    </dl>
  )
}

/**
 * A declared local copy under its reference: its newest reading, what it says against what is
 * declared, and Read when this computer is the copy's workstation.
 */
export function LocalCopyReading({ copy }: { copy: LocalCopy }) {
  const { snapshot, now, thisWorkstation, workstationError, readCopies, readingCopies } =
    useInventory()
  const [outcome, setOutcome] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const here = onThisComputer(copy, thisWorkstation)
  const running = readingCopies.includes(copy.referenceId)
  const latest = copy.latest
  const owner =
    copy.subject.type === 'application'
      ? snapshot?.applications.find((item) => item.id === copy.subject.id)?.name
      : snapshot?.resources.find((item) => item.id === copy.subject.id)?.name
  const read = async () => {
    setOutcome(null)
    setError(null)
    try {
      const result = await readCopies([copy.referenceId])
      const failure = result.failures[0]
      if (failure) setError(`${failure.message} Nothing was kept.`)
      else setOutcome('Read and kept. What it found is above.')
    } catch (cause) {
      setError(inventoryError(cause))
    }
  }
  return (
    <div className="mt-2 space-y-1.5">
      {latest ? (
        <>
          <ReadingHeadline reading={latest} now={now} />
          <ReadingFacts reading={latest} now={now} />
          {copy.movedSinceRead && (
            <p className="text-[11px] break-words text-amber-200/90">
              This reading was made at <span className="font-mono">{latest.path}</span> on{' '}
              <MachineName id={latest.machineId} />; the copy is declared elsewhere now.
            </p>
          )}
          {copy.repository === 'differs' && (
            <p className="text-[11px] text-amber-200/90">
              None of its remotes looked like the repository {owner ?? 'its system'} declares.
            </p>
          )}
          {copy.repository === 'undeclared' && (
            <p className="text-[11px] text-muted-foreground">
              It had remotes, and {owner ?? 'its system'} declares no repository.
            </p>
          )}
          {copy.repository === 'matches' && (
            <p className="text-[11px] text-muted-foreground">
              A remote pointed to the repository {owner ?? 'its system'} declares.
            </p>
          )}
        </>
      ) : (
        <p className="text-[11px] text-muted-foreground">
          <FolderGit2 className="mr-1 inline size-3" aria-hidden="true" />
          Never read: DevHQ knows nothing of what this copy holds.
        </p>
      )}
      {here ? (
        <div className="flex flex-wrap items-center gap-2 pt-0.5">
          <Button
            size="sm"
            variant="outline"
            disabled={running}
            aria-label={`Read ${copy.path}`}
            onClick={() => void read()}
          >
            <ScanSearch className="size-3.5" aria-hidden="true" />
            {running ? 'Reading…' : 'Read'}
          </Button>
          <p role="status" className="text-[11px] text-muted-foreground empty:hidden">
            {running
              ? 'Reading, without running anything it names, for up to 30 s. You can keep working.'
              : outcome}
          </p>
        </div>
      ) : thisWorkstation?.resourceId ? (
        <p className="text-[11px] text-muted-foreground">
          DevHQ on <MachineName id={copy.machineId} /> reads it; this computer is{' '}
          <MachineName id={thisWorkstation.resourceId} />.
        </p>
      ) : (
        <p className="text-[11px] text-muted-foreground">
          DevHQ reads only copies on this computer, and does not know which workstation it is:{' '}
          {workstationError ? `${workstationError} ` : ''}
          <Link className="text-primary hover:underline" to="/settings">
            choose it in Settings
          </Link>
          .
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs break-words text-rose-300">
          {error}
        </p>
      )}
    </div>
  )
}

/**
 * Every reading of a system's copies, newest first and a page at a time, behind a toggle: closed,
 * it reads nothing; closing it forgets what it loaded.
 */
export function ReadingHistoryToggle({ subject, name }: { subject: EntityRef; name: string }) {
  const [open, setOpen] = useState(false)
  const region = useId()
  return (
    <div>
      <Button
        size="sm"
        variant="ghost"
        className="-ml-2"
        aria-expanded={open}
        aria-controls={region}
        onClick={() => setOpen(!open)}
      >
        <ChevronDown
          className={cn('size-3.5 transition-transform', open && 'rotate-180')}
          aria-hidden="true"
        />
        {open ? 'Hide readings' : 'Show every reading'}
      </Button>
      <div
        id={region}
        role="region"
        aria-label={`Readings of ${name}`}
        hidden={!open}
        className="mt-3"
      >
        {open && <ReadingHistory subject={subject} name={name} />}
      </div>
    </div>
  )
}

function ReadingHistory({ subject, name }: { subject: EntityRef; name: string }) {
  const { now, olderReadings, snapshot } = useInventory()
  const [readings, setReadings] = useState<CopyReading[] | null>(null)
  const [more, setMore] = useState(false)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const loadOlder = async (before: string) => {
    setLoading(true)
    setError(null)
    try {
      const page = await olderReadings(subject, before)
      setReadings((current) => {
        const seen = new Set((current ?? []).map((item) => item.id))
        return [...(current ?? []), ...page.readings.filter((item) => !seen.has(item.id))]
      })
      setMore(page.more)
    } catch (cause) {
      setError(inventoryError(cause))
    } finally {
      setLoading(false)
    }
  }
  // The first page, once, when opened. Keyed by the system, not by the object naming it, which
  // each render of the page makes anew.
  const { type, id } = subject
  useEffect(() => {
    let current = true
    olderReadings({ type, id } as EntityRef, null).then(
      (page) => {
        if (!current) return
        setReadings(page.readings)
        setMore(page.more)
      },
      (cause) => current && setError(inventoryError(cause)),
    )
    return () => {
      current = false
    }
  }, [olderReadings, type, id])
  const declared = new Set(snapshot?.localCopies.map((copy) => copy.referenceId))
  return (
    <div className="space-y-3">
      {readings === null && !error && (
        <p role="status" className="text-[11px] text-muted-foreground">
          Loading readings…
        </p>
      )}
      {readings?.length === 0 && (
        <p className="text-xs text-muted-foreground">
          No copy of {name} has been read. Its copies’ readings appear here once DevHQ reads them.
        </p>
      )}
      {!!readings?.length && (
        <ol className="divide-y" aria-label={`Readings of ${name}, newest first`}>
          {readings.map((reading) => (
            <li key={reading.id} className="py-3 first:pt-0">
              <p className="mb-1 font-mono text-[11px] break-all text-foreground/90">
                {reading.path}
                {!declared.has(reading.referenceId) && (
                  <span className="font-sans text-muted-foreground"> · no longer declared</span>
                )}
              </p>
              <ReadingHeadline reading={reading} now={now} />
              <p className="mt-1 text-[11px] leading-5 break-words text-muted-foreground">
                {readingSummary(reading)}
              </p>
            </li>
          ))}
        </ol>
      )}
      {more && readings && (
        <>
          <p className="text-[11px] text-muted-foreground">
            The newest {counted(readings.length, 'reading')} are shown; older ones are kept and load{' '}
            {readingsPage} at a time.
          </p>
          <Button
            size="sm"
            variant="outline"
            disabled={loading}
            onClick={() => void loadOlder(readings.at(-1)!.id)}
          >
            {loading ? 'Loading older readings…' : `Show ${readingsPage} older readings`}
          </Button>
        </>
      )}
      {readings && readings.length > 0 && !more && (
        <p className="text-[11px] text-muted-foreground">Every reading of {name} is shown.</p>
      )}
      {error && (
        <p role="alert" className="text-xs text-rose-300">
          {error}
        </p>
      )}
    </div>
  )
}
