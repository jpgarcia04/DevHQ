import { Button } from '@/components/ui/button'
import { referenceKindLabels } from '@/domain/labels'
import type { EntityRef } from '@/domain/models'
import { useCopy } from './copy'
import { LocalCopyReading, ReadingHistoryToggle } from './copy-readings'
import { SubjectLink } from './evidence'
import type { InventoryReference } from './inventory-api'
import { useInventory } from './inventory-context'
import { sameRepositoryAs } from './references'

/**
 * Where a system is: each reference with the machine a path is on, and a button that copies its
 * URL or path. DevHQ opens and checks none of them, but reads a local copy when asked: each one
 * shows its newest reading, and the system's earlier readings load on demand. A system without
 * references is not a problem to solve, nor a gap in what DevHQ observes: it says so plainly.
 */
export function ReferenceList({
  subject,
  references,
}: {
  subject: EntityRef
  references: InventoryReference[]
}) {
  const { snapshot } = useInventory()
  const { copied, copy } = useCopy()
  if (!references.length) return <p className="text-xs text-muted-foreground">No references yet.</p>
  const name = (
    subject.type === 'application' ? snapshot?.applications : snapshot?.resources
  )?.find((item) => item.id === subject.id)?.name
  const copies = snapshot?.localCopies ?? []
  return (
    <>
      <ul className="divide-y" aria-label="References">
        {references.map((reference) => {
          const shared = snapshot ? sameRepositoryAs(snapshot, subject, reference) : []
          const localCopy =
            reference.kind === 'local_copy'
              ? copies.find((item) => item.referenceId === reference.id)
              : undefined
          return (
            <li
              className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 py-3 text-xs first:pt-0 last:pb-0"
              key={reference.id}
            >
              <div className="min-w-0 flex-1">
                <p className="font-medium">
                  {referenceKindLabels[reference.kind]}
                  {reference.label && (
                    <span className="font-normal text-muted-foreground"> · {reference.label}</span>
                  )}
                </p>
                <p className="mt-1 select-all break-all font-mono text-[11px] text-foreground/90">
                  {reference.target}
                </p>
                {reference.machineId && (
                  <p className="mt-1 text-[11px] text-muted-foreground">
                    On <SubjectLink subject={{ type: 'resource', id: reference.machineId }} />
                  </p>
                )}
                {shared.length > 0 && (
                  <p className="mt-1 text-[11px] text-muted-foreground">
                    Same repository as{' '}
                    {shared.map((system, index) => (
                      <span key={`${system.type}:${system.id}`}>
                        {index > 0 && ', '}
                        <SubjectLink subject={system} />
                      </span>
                    ))}
                  </p>
                )}
                {localCopy && <LocalCopyReading copy={localCopy} />}
              </div>
              <div className="flex shrink-0 flex-wrap items-center gap-3">
                <p role="status" className="text-[11px] text-muted-foreground">
                  {copied?.key === reference.id && (
                    <span key={copied.at} className="fade-in">
                      {copied.ok ? 'Copied.' : 'It could not be copied: select it and copy it.'}
                    </span>
                  )}
                </p>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={`Copy ${reference.target}`}
                  onClick={() => void copy(reference.target, reference.id)}
                >
                  Copy
                </Button>
              </div>
            </li>
          )
        })}
      </ul>
      {references.some((reference) => reference.kind === 'local_copy') && (
        <div className="mt-3 border-t pt-3">
          <ReadingHistoryToggle subject={subject} name={name ?? 'this system'} />
        </div>
      )}
    </>
  )
}
