import { sameSubject } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import { counted } from '@/shared/count'
import { formatDate, relativeTime } from '@/shared/time'
import type { CopyReading, InventorySnapshot, LocalCopy, ThisWorkstation } from './inventory-api'

/** The native process caps unpublished commits at 1000 and says 1001 for more. */
export const unpublishedLimit = 1000

/** A count of unpublished commits as people read it: 1001 is «1000+». */
export function unpublishedText(count: number) {
  return count > unpublishedLimit ? `${unpublishedLimit}+` : String(count)
}

/** Whether a system is an archived application: its copies leave Read all and the panel. */
export function archived(snapshot: InventorySnapshot, subject: EntityRef) {
  return (
    subject.type === 'application' &&
    snapshot.applications.some((app) => app.id === subject.id && app.lifecycle === 'archived')
  )
}

/** The workstation this computer is, when DevHQ knows it. */
export function thisMachine(workstation: ThisWorkstation | null) {
  return workstation?.resourceId ?? null
}

/** Whether a copy is on this computer, so that this DevHQ can read it. */
export function onThisComputer(copy: LocalCopy, workstation: ThisWorkstation | null) {
  const machine = thisMachine(workstation)
  return machine !== null && copy.machineId === machine
}

/**
 * The copies Read all reads, as the native process chooses them: every copy of this workstation
 * but those of archived applications. Empty when DevHQ does not know which workstation this is.
 */
export function readAllTargets(snapshot: InventorySnapshot, workstation: ThisWorkstation | null) {
  return snapshot.localCopies.filter(
    (copy) => onThisComputer(copy, workstation) && !archived(snapshot, copy.subject),
  )
}

/** A system's local copies as declared now, each with its newest reading. */
export function copiesOf(snapshot: InventorySnapshot, subject: EntityRef) {
  return snapshot.localCopies.filter((copy) => sameSubject(copy.subject, subject))
}

export interface LastActivity {
  /** Copies the system declares now. */
  copies: number
  /** Of those, the ones never read: activity is at least what the others say, never less. */
  neverRead: number
  /** The newest commit any latest reading found, with that reading; null when none found one. */
  lastCommitAt: string | null
  basis: CopyReading | null
}

/**
 * When a system last saw a commit, as far as the newest reading of each of its copies says: the
 * newest commit among them. A lower bound: a copy never read, or read long ago, may hold newer
 * work, so a system is never called inactive. The agent gate derives the same (`local_activity`).
 */
export function lastActivity(snapshot: InventorySnapshot, subject: EntityRef): LastActivity {
  const copies = copiesOf(snapshot, subject)
  let basis: CopyReading | null = null
  for (const copy of copies) {
    const found = copy.latest?.lastCommitAt
    if (found && (!basis?.lastCommitAt || Date.parse(found) > Date.parse(basis.lastCommitAt)))
      basis = copy.latest
  }
  return {
    copies: copies.length,
    neverRead: copies.filter((copy) => !copy.latest).length,
    lastCommitAt: basis?.lastCommitAt ?? null,
    basis,
  }
}

/**
 * What Overview says of the local copies, beside «what needs my attention?» and never in it: they
 * are not health. Copies of archived applications are left out. The same lists as the agent gate's
 * `localCopies` in `attention`.
 */
export function localCopiesOverview(
  snapshot: InventorySnapshot,
  workstation: ThisWorkstation | null,
) {
  const current = snapshot.localCopies.filter((copy) => !archived(snapshot, copy.subject))
  const machine = thisMachine(workstation)
  return {
    current,
    /** Held commits on no remote, stashes, changes, new or unverifiable files, or an operation. */
    work: current.filter((copy) => copy.localWork),
    /** Not where declared, not a copy, or not readable, when last read. */
    notFound: current.filter((copy) => copy.latest && copy.latest.outcome !== 'read'),
    neverReadHere: current.filter((copy) => !copy.latest && copy.machineId === machine),
    /** Never read on other computers, or anywhere while DevHQ does not know this one. */
    neverReadElsewhere: current.filter((copy) => !copy.latest && copy.machineId !== machine),
  }
}

/**
 * A system's last activity in words, with what it rests on: «Last commit 3 days ago, read on Laptop
 * 2 hours ago · 1 copy never read». Never «inactive»: copies not read, or read long ago, may hold
 * newer work.
 */
export function activityText(
  activity: LastActivity,
  machineName: (id: string) => string,
  now: number,
) {
  const { copies, neverRead, lastCommitAt, basis } = activity
  if (!copies) return 'No local copy declared'
  const unread = neverRead ? ` · ${counted(neverRead, 'copy', 'copies')} never read` : ''
  if (!lastCommitAt || !basis)
    return neverRead === copies
      ? `Activity unknown: ${copies === 1 ? 'its copy was never read' : `none of its ${copies} copies was read`}`
      : `No commit found in the latest readings${unread}`
  const when = (time: string) =>
    Date.parse(time) > now ? `dated ${formatDate(time)}` : relativeTime(time, now).toLowerCase()
  return `Last commit ${when(lastCommitAt)}, read on ${machineName(basis.machineId)} ${when(basis.observedAt)}${unread}`
}
