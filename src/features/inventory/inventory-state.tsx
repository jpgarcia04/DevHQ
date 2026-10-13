import { EmptyState } from '@/components/empty-state'
import { Button } from '@/components/ui/button'
import { useInventory } from './inventory-context'

/**
 * What the window found instead of an inventory it serves. DevHQ decides it once, when it starts,
 * and never opens such a file again until it starts again: reading it again would answer the same,
 * so these offer no Retry.
 */
const refusedUntilRestart: Record<string, { title: string; next: string }> = {
  schema_outdated: {
    title: 'Inventory needs a schema upgrade',
    next: 'Settings shows where the file and its backups folder are, so that you can keep a copy of it.',
  },
  schema_newer: {
    title: 'Inventory from a newer DevHQ',
    next: 'Open it with the DevHQ that wrote it.',
  },
  schema_mismatch: {
    title: 'Inventory unavailable',
    next: 'DevHQ looks at the file again the next time it starts.',
  },
  not_an_inventory: {
    title: 'Inventory unavailable',
    next: 'DevHQ looks at the file again the next time it starts.',
  },
  leftover_files: {
    title: 'Inventory unavailable',
    next: 'DevHQ looks at the folder again the next time it starts.',
  },
}

export function InventoryState({ children }: { children: React.ReactNode }) {
  const { loadState, error, errorCode, reload, snapshot } = useInventory()
  // A reload after a save keeps the previous snapshot visible, so open forms and
  // expanded records survive it. A failed reload clears the snapshot and shows the error.
  if (loadState === 'idle' || (loadState === 'loading' && !snapshot))
    return (
      <p role="status" className="py-8 text-sm text-muted-foreground">
        Loading local inventory…
      </p>
    )
  if (loadState === 'error') {
    const refused = errorCode ? refusedUntilRestart[errorCode] : undefined
    if (refused)
      return (
        <EmptyState
          title={refused.title}
          description={`${error ?? 'DevHQ serves nothing from this file.'} ${refused.next}`}
        />
      )
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
  }
  return children
}
