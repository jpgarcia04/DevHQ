import { subjectsOf } from '@/domain/evidence'
import { counted } from '@/shared/count'
import { relativeTime } from '@/shared/time'
import { MachineName } from './copy-readings'
import { SubjectLink } from './evidence'
import { declaredVersion } from './installations'
import type {
  Installation,
  InstallationsRead,
  InventorySnapshot,
  LocalCopy,
  ReadOutcome,
} from './inventory-api'
import { useInventory } from './inventory-context'
import type { OpeningRead } from './inventory-context'
import { unpublishedText } from './local-copies'
import { oldReadingMs, pendingWorkOf, type PendingItem, type PendingReason } from './pending'

const oldReadingDays = oldReadingMs / (24 * 60 * 60 * 1000)

/** One reason in a few words, from the reading it rests on. */
function reasonText(
  reason: PendingReason,
  snapshot: InventorySnapshot,
  copy: LocalCopy | undefined,
  installation: Installation | undefined,
) {
  const reading = copy?.latest
  switch (reason) {
    case 'operation':
      return `a ${reading?.operation?.replace('_', '-') ?? 'git operation'} left halfway`
    case 'changes':
      return [
        reading?.changed ? counted(reading.changed, 'changed file') : '',
        reading?.untracked ? counted(reading.untracked, 'new entry', 'new entries') : '',
        reading?.unverifiable ? counted(reading.unverifiable, 'unverifiable file') : '',
      ]
        .filter(Boolean)
        .join(', ')
    case 'unpublished': {
      const commits = (reading?.unpublishedOnHead ?? 0) + (reading?.unpublishedElsewhere ?? 0)
      return `${unpublishedText(commits)} ${commits === 1 ? 'commit' : 'commits'} on no remote`
    }
    case 'stashes':
      return counted(reading?.stashes ?? 0, 'stash', 'stashes')
    case 'version_differs':
      return `the file declared ${installation?.latest?.productVersion ?? 'a version'}, the inventory ${
        installation ? (declaredVersion(snapshot, installation) ?? 'none') : 'another'
      }`
    case 'not_found':
      return 'not found where declared, or not readable'
    case 'never_read':
      return 'never read on this computer'
    case 'old_reading':
      return `read more than ${counted(oldReadingDays, 'day')} ago`
  }
}

function PendingRow({ item }: { item: PendingItem }) {
  const { snapshot, now } = useInventory()
  if (!snapshot) return null
  const copy =
    item.kind === 'localCopy'
      ? snapshot.localCopies.find((each) => each.referenceId === item.referenceId)
      : undefined
  const installation =
    item.kind === 'installation'
      ? snapshot.installations.find((each) => each.referenceId === item.referenceId)
      : undefined
  const declared = copy ?? installation
  const reading = copy?.latest ?? installation?.latest
  const reasons = item.reasons.map((reason) => reasonText(reason, snapshot, copy, installation))
  return (
    <li className="space-y-0.5 px-4 py-3">
      <p className="flex flex-wrap items-center gap-x-2">
        <SubjectLink subject={item.subject} className="min-w-0 truncate text-foreground" />
        <span className="text-[11px] text-muted-foreground">
          {item.kind === 'localCopy' ? 'Local copy' : 'Installation'}
          {declared && (
            <>
              {' '}
              on <MachineName id={declared.machineId} />
            </>
          )}
        </span>
      </p>
      {declared && (
        <p className="font-mono text-[11px] break-all text-muted-foreground">{declared.path}</p>
      )}
      <p className="text-[11px] text-foreground/85">
        {reasons.join(' · ').replace(/^./, (first) => first.toUpperCase())}
      </p>
      {reading && (
        <p className="text-[11px] text-muted-foreground">
          Read {relativeTime(reading.observedAt, now).toLowerCase()}
          {reading.agent ? `, asked by ${reading.agent}` : ''}
        </p>
      )}
    </li>
  )
}

/** What one side of the opening read did, in a few words. */
function sideText(
  outcome: ReadOutcome | InstallationsRead | string | null,
  one: string,
  many: string,
) {
  if (outcome === null) return null
  if (typeof outcome === 'string')
    return `failed to read the ${many} (${outcome.replace(/\.$/, '')})`
  const failed = outcome.failures.length
  return `read ${counted(outcome.readings.length, one, many)}${failed ? ` (${failed} could not be read and kept nothing)` : ''}`
}

function openingText(read: OpeningRead | null) {
  if (!read) return null
  if (read.state === 'reading')
    return "Reading this computer's local copies and installations, as DevHQ does when it opens. You can keep working."
  const sides = [
    sideText(read.copies, 'local copy', 'local copies'),
    sideText(read.installations, 'installation', 'installations'),
  ].filter(Boolean)
  return sides.length ? `When DevHQ opened, it ${sides.join('; ')}.` : null
}

/**
 * Beside «what needs my attention?» and never in it: what the readings of local copies and
 * installations say is waiting, most urgent first (see `pending.ts`). The agent gate gives the same
 * list as `pendingWork` in `attention`.
 */
export function PendingWorkOverview() {
  const { snapshot, thisWorkstation, now, openingRead } = useInventory()
  if (!snapshot || !subjectsOf(snapshot).length) return null
  const items = pendingWorkOf(snapshot, thisWorkstation, now)
  const declared = snapshot.localCopies.length + snapshot.installations.length
  const opening = openingText(openingRead)
  return (
    <section aria-labelledby="pending-work-heading">
      <h2 id="pending-work-heading" className="section-heading mb-3">
        Pending work{' '}
        <span className="ml-2 rounded bg-sky-300/10 px-1.5 py-0.5 font-mono text-[10px] text-sky-200">
          {items.length}
        </span>
      </h2>
      <div className="panel overflow-hidden text-xs">
        {opening && (
          <p
            role="status"
            className="border-b px-4 py-2.5 text-[11px] break-words text-muted-foreground"
          >
            {opening}
          </p>
        )}
        {items.length ? (
          <ul className="divide-y">
            {items.map((item) => (
              <PendingRow key={`${item.kind}:${item.referenceId}`} item={item} />
            ))}
          </ul>
        ) : (
          <p className="px-5 py-6 text-center text-sm text-muted-foreground">
            {declared
              ? 'No reading says work is waiting in a local copy or an installation.'
              : 'No local copy or installation is declared. DevHQ reads only what the inventory names: add them as references of an application or resource.'}
          </p>
        )}
        <p className="border-t px-4 py-2.5 text-[11px] leading-5 text-muted-foreground">
          Work left halfway, uncommitted or on no remote first, then installations that declared
          another version, then what was not found, never read or read more than{' '}
          {counted(oldReadingDays, 'day')} ago on this computer. What each copy or file held when it
          was read, which may no longer be so. Not health: none of it is in Needs attention.
        </p>
      </div>
    </section>
  )
}
