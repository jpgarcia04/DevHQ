import { sameSubject } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import type { InventorySnapshot } from './inventory-api'

/**
 * Why a subject cannot be deleted, or null if it can. DevHQ never erases history, and a
 * resource that components run on would leave them pointing nowhere. The native process
 * checks the same rules again before deleting anything.
 */
export function deletionBlocker(snapshot: InventorySnapshot, subject: EntityRef): string | null {
  if (subject.type === 'resource') {
    const users = snapshot.applications.flatMap((app) =>
      app.components
        .filter((part) => part.resourceId === subject.id)
        .map((part) => `${app.name} (${part.name})`),
    )
    if (users.length)
      return `Used by ${users.join(', ')}. Link those components to another resource, or leave them unlinked, first.`
  }
  const about = (item: { subject: EntityRef }) => sameSubject(item.subject, subject)
  if (
    snapshot.observations.some(about) ||
    snapshot.incidents.some(about) ||
    snapshot.events.some(about)
  )
    return subject.type === 'application'
      ? 'It has recorded observations, incidents or events. DevHQ keeps history, so it cannot be deleted; set its lifecycle to Archived instead.'
      : 'It has recorded observations, incidents or events. DevHQ keeps history, so it cannot be deleted.'
  return null
}
