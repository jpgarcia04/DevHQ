import { sameSubject } from '@/domain/evidence'
import { referenceKindLabels } from '@/domain/labels'
import type { EntityRef } from '@/domain/models'
import type { InventorySnapshot } from './inventory-api'

/**
 * Why a subject cannot be deleted, or null if it can. DevHQ never erases history; a resource
 * that components run on would leave them pointing nowhere, and one that holds other systems'
 * paths would change their configuration. The native process checks the same rules again
 * before deleting anything.
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
    // Its own paths go with it.
    const systems = [
      ...snapshot.applications,
      ...snapshot.resources.filter((resource) => resource.id !== subject.id),
    ]
    const paths = systems.flatMap((system) =>
      system.references
        .filter((reference) => reference.machineId === subject.id)
        .map(
          (reference) =>
            `${system.name} (${referenceKindLabels[reference.kind].toLowerCase()} ${reference.target})`,
        ),
    )
    if (paths.length)
      return `Holds paths of ${paths.join(', ')}. Remove those references, or put them on another machine, first.`
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
