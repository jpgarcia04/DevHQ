import { sameSubject } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import type { InventoryReference, InventorySnapshot, ReferenceInput } from './inventory-api'

/** A web address, told from a path as the native process tells it: http:// or https://. */
export const isWebAddress = (target: string) => /^https?:\/\//i.test(target.trim())

/**
 * The other systems that declare the same repository (the parts of a monorepo, say), as the native
 * process derived them when it read the inventory: by exact equality of the URL.
 */
export function sameRepositoryAs(
  snapshot: InventorySnapshot,
  subject: EntityRef,
  reference: InventoryReference,
): EntityRef[] {
  if (reference.kind !== 'repository') return []
  return (
    snapshot.sharedRepositories
      .find((item) => item.target === reference.target)
      ?.systems.filter((system) => !sameSubject(system, subject)) ?? []
  )
}

/** What a form sends: no machine for a URL, and no label when it is blank. */
export const referenceInputs = (drafts: ReferenceInput[]): ReferenceInput[] =>
  drafts.map((draft) => ({
    ...draft,
    label: draft.label?.trim() ? draft.label : null,
    machineId: isWebAddress(draft.target) ? null : draft.machineId,
  }))
