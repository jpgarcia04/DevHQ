import type { EntityRef } from '@/domain/models'
import type { InventorySnapshot, ThisWorkstation } from './inventory-api'
import { thisMachine } from './local-copies'

/**
 * «What work is waiting for me?»: what the readings of local copies and installations say needs
 * looking at, ranked. The agent gate answers it in `src-tauri/src/pending.rs`; both are held to the
 * same fixtures in `contract/pending-work/`, so they can never disagree.
 *
 * Not health: it never enters Needs attention or coverage. A reading says what a copy or a file held
 * when it was read, which may no longer be so, and each item is shown with that moment.
 *
 * - Each declared copy or installation gives at most one item, with every reason it has, most urgent
 *   first; the item ranks by its first reason.
 * - Ties keep the order given: copies in the inventory's order, then installations in theirs.
 * - Systems of archived applications are left out, as from Read all.
 */

/** A reading on this workstation older than this is worth renewing: three days. */
export const oldReadingMs = 3 * 24 * 60 * 60 * 1000

/** Every reason, most urgent first: an item ranks by the position of its first one. */
export const pendingReasons = [
  'operation',
  'changes',
  'unpublished',
  'stashes',
  'version_differs',
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
}

export interface PendingItem {
  kind: 'localCopy' | 'installation'
  referenceId: string
  subject: EntityRef
  /** Most urgent first; never empty. */
  reasons: PendingReason[]
}

const some = (count: number | null) => count !== null && count > 0

/**
 * The pending work among the copies and installations given, most urgent first, at `now`. `machine`
 * is the workstation this computer is, if DevHQ knows it; `archived` names archived applications.
 */
export function pendingWork(
  copies: CopyFacts[],
  installations: InstallationFacts[],
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
      else if (installation.version === 'differs') reasons.push('version_differs')
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
    thisMachine(workstation),
    snapshot.applications.filter((app) => app.lifecycle === 'archived').map((app) => app.id),
    now,
  )
}
