import type { EntityRef } from '@/domain/models'
import type { InventorySnapshot, ThisWorkstation } from './inventory-api'
import { thisMachine } from './local-copies'

/**
 * «What work is waiting for me?»: what the readings of local copies, installations and repositories
 * on GitHub say needs looking at, ranked. The agent gate answers it in `src-tauri/src/pending.rs`; both are held to the
 * same fixtures in `contract/pending-work/`, so they can never disagree.
 *
 * Not health: it never enters Needs attention or coverage. A reading says what a copy or a file held
 * when it was read, which may no longer be so, and each item is shown with that moment.
 *
 * - Each declared copy, installation or repository gives at most one item, with every reason it has,
 *   most urgent first; the item ranks by its first reason.
 * - A repository is on no workstation: it is never read or old wherever DevHQ runs. Its pull request
 *   is waiting when it had not changed for more than `waitingMs` at the reading, not now.
 * - Ties keep the order given: copies in the inventory's order, then installations, then
 *   repositories, each in theirs.
 * - Systems of archived applications are left out, as from Read all.
 */

/** A reading on this workstation older than this is worth renewing: three days. */
export const oldReadingMs = 3 * 24 * 60 * 60 * 1000

/** A pull request unchanged for longer than this when its repository was read is waiting. */
export const waitingMs = 7 * 24 * 60 * 60 * 1000

/** The conclusions of a run that did not pass. */
export const failedConclusions = ['failure', 'timed_out', 'startup_failure']

/** Every reason, most urgent first: an item ranks by the position of its first one. */
export const pendingReasons = [
  'main_failing',
  'operation',
  'changes',
  'unpublished',
  'stashes',
  'pr_waiting',
  'version_differs',
  'not_installed',
  'not_found',
  'never_read',
  'old_reading',
] as const

export type PendingReason = (typeof pendingReasons)[number]

interface CopyFacts {
  referenceId: string
  subject: EntityRef
  machineId: string
  latest: {
    outcome: string
    observedAt: string
    unpublishedOnHead: number | null
    unpublishedElsewhere: number | null
    stashes: number | null
    changed: number | null
    untracked: number | null
    unverifiable: number | null
    operation: string | null
  } | null
}

interface InstallationFacts {
  referenceId: string
  subject: EntityRef
  machineId: string
  latest: { outcome: string; observedAt: string } | null
  version: string | null
  /** The state of how the installed build compares with a local copy, when compared. */
  againstCopy?: { state: string } | null
}

interface RepositoryFacts {
  referenceId: string
  subject: EntityRef
  latest: {
    outcome: string
    observedAt: string
    workflows: { conclusion: string }[] | null
    pullRequests: { updatedAt: string }[] | null
  } | null
}

export interface PendingItem {
  kind: 'localCopy' | 'installation' | 'repository'
  referenceId: string
  subject: EntityRef
  /** Most urgent first; never empty. */
  reasons: PendingReason[]
}

const some = (count: number | null) => count !== null && count > 0

/**
 * The pending work among the copies, installations and repositories given, most urgent first, at
 * `now`. `machine`
 * is the workstation this computer is, if DevHQ knows it; `archived` names archived applications.
 */
export function pendingWork(
  copies: CopyFacts[],
  installations: InstallationFacts[],
  repositories: RepositoryFacts[],
  machine: string | null,
  archived: string[],
  now: number,
): PendingItem[] {
  const leftOut = (subject: EntityRef) =>
    subject.type === 'application' && archived.includes(subject.id)
  const old = (observedAt: string, here: boolean) =>
    here && now - Date.parse(observedAt) > oldReadingMs
  const items: PendingItem[] = []
  for (const copy of copies) {
    if (leftOut(copy.subject)) continue
    const here = machine !== null && copy.machineId === machine
    const reasons: PendingReason[] = []
    const reading = copy.latest
    if (!reading) {
      if (here) reasons.push('never_read')
    } else {
      if (reading.outcome === 'read') {
        if (reading.operation !== null) reasons.push('operation')
        if (some(reading.changed) || some(reading.untracked) || some(reading.unverifiable))
          reasons.push('changes')
        if (some(reading.unpublishedOnHead) || some(reading.unpublishedElsewhere))
          reasons.push('unpublished')
        if (some(reading.stashes)) reasons.push('stashes')
      } else reasons.push('not_found')
      if (old(reading.observedAt, here)) reasons.push('old_reading')
    }
    if (reasons.length)
      items.push({
        kind: 'localCopy',
        referenceId: copy.referenceId,
        subject: copy.subject,
        reasons,
      })
  }
  for (const installation of installations) {
    if (leftOut(installation.subject)) continue
    const here = machine !== null && installation.machineId === machine
    const reasons: PendingReason[] = []
    const reading = installation.latest
    if (!reading) {
      if (here) reasons.push('never_read')
    } else {
      if (reading.outcome !== 'read') reasons.push('not_found')
      else {
        if (installation.version === 'differs') reasons.push('version_differs')
        const against = installation.againstCopy?.state
        if (against === 'other_commit' || against === 'newer_commits') reasons.push('not_installed')
      }
      if (old(reading.observedAt, here)) reasons.push('old_reading')
    }
    if (reasons.length)
      items.push({
        kind: 'installation',
        referenceId: installation.referenceId,
        subject: installation.subject,
        reasons,
      })
  }
  for (const repository of repositories) {
    if (leftOut(repository.subject)) continue
    const reasons: PendingReason[] = []
    const reading = repository.latest
    if (!reading) reasons.push('never_read')
    else {
      if (reading.outcome === 'read') {
        if ((reading.workflows ?? []).some((run) => failedConclusions.includes(run.conclusion)))
          reasons.push('main_failing')
        const readAt = Date.parse(reading.observedAt)
        if (
          (reading.pullRequests ?? []).some(
            (pull) => readAt - Date.parse(pull.updatedAt) > waitingMs,
          )
        )
          reasons.push('pr_waiting')
      } else reasons.push('not_found')
      if (old(reading.observedAt, true)) reasons.push('old_reading')
    }
    if (reasons.length)
      items.push({
        kind: 'repository',
        referenceId: repository.referenceId,
        subject: repository.subject,
        reasons,
      })
  }
  // Array.prototype.sort is stable: equal ranks keep the order they were gathered in.
  const rank = (item: PendingItem) => pendingReasons.indexOf(item.reasons[0]!)
  return items.sort((a, b) => rank(a) - rank(b))
}

/** The pending work of an inventory, as Overview shows it and the agent gate gives it. */
export function pendingWorkOf(
  snapshot: InventorySnapshot,
  workstation: ThisWorkstation | null,
  now: number,
) {
  return pendingWork(
    snapshot.localCopies,
    snapshot.installations,
    snapshot.githubRepositories,
    thisMachine(workstation),
    snapshot.applications.filter((app) => app.lifecycle === 'archived').map((app) => app.id),
    now,
  )
}
