import { EmptyState } from '@/components/empty-state'
import { Button } from '@/components/ui/button'
import { useInventory } from './inventory-context'

export function InventoryState({ children }: { children: React.ReactNode }) {
  const { loadState, error, reload, snapshot } = useInventory()
  // A reload after a save keeps the previous snapshot visible, so open forms and
  // expanded records survive it. A failed reload clears the snapshot and shows the error.
  if (loadState === 'idle' || (loadState === 'loading' && !snapshot))
    return (
      <p role="status" className="py-8 text-sm text-muted-foreground">
        Loading local inventory…
      </p>
    )
  if (loadState === 'error')
    return (
      <EmptyState
        title="Inventory unavailable"
        description={error ?? 'The inventory request failed.'}
        action={
          <Button variant="outline" onClick={() => void reload()}>
            Retry
          </Button>
        }
      />
    )
  return children
}
