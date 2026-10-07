import { useState } from 'react'
import { Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type { EntityRef } from '@/domain/models'
import { deletionBlocker } from './deletion'
import { inventoryError } from './inventory-api'
import { useInventory } from './inventory-context'

/** Deleting an application or resource, for one that has nothing worth keeping yet. */
export function DeleteSubject({
  subject,
  name,
  onDeleted,
}: {
  subject: EntityRef
  name: string
  onDeleted: () => void
}) {
  const { snapshot, deleteApplication, deleteResource } = useInventory()
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const label = subject.type === 'application' ? 'application' : 'resource'
  const blocker = snapshot ? deletionBlocker(snapshot, subject) : null
  const remove = async () => {
    setBusy(true)
    setError(null)
    try {
      await (subject.type === 'application' ? deleteApplication : deleteResource)(subject.id)
      onDeleted()
    } catch (cause) {
      setError(inventoryError(cause))
      setBusy(false)
    }
  }
  return (
    <section aria-label={`Delete ${name}`} className="space-y-3 border-t pt-4">
      <p className="text-[11px] leading-5 text-muted-foreground">
        {blocker ??
          `Deleting removes ${name}${subject.type === 'application' ? ', its components' : ''}, its references and its HTTP check for good. Only something without recorded history can be deleted.`}
      </p>
      {confirming ? (
        <div
          role="group"
          aria-label="Confirm deletion"
          className="flex flex-wrap items-center gap-2"
        >
          <span className="text-xs">Delete {name}? This cannot be undone.</span>
          <Button
            type="button"
            size="sm"
            variant="destructive"
            disabled={busy}
            onClick={() => void remove()}
          >
            {busy ? 'Deleting…' : `Delete ${label}`}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => setConfirming(false)}
          >
            Keep it
          </Button>
        </div>
      ) : (
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={!!blocker}
          onClick={() => setConfirming(true)}
        >
          <Trash2 className="size-3.5" aria-hidden="true" /> Delete {label}…
        </Button>
      )}
      {error && (
        <p role="alert" className="text-xs text-rose-300">
          {error}
        </p>
      )}
    </section>
  )
}
