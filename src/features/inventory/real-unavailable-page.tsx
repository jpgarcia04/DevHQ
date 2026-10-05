import { EmptyState } from '@/components/empty-state'
import { PageHeader } from '@/components/page-header'
import { InventoryState } from './inventory-state'

export function RealUnavailablePage({ title }: { title: string }) {
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory"
        title={title}
        description="This operational area has no observations yet."
      />
      <InventoryState>
        <EmptyState
          title={`No real ${title.toLowerCase()} records`}
          description="DevHQ has not collected events, incidents or integration results. An empty record set does not establish health."
        />
      </InventoryState>
    </div>
  )
}
