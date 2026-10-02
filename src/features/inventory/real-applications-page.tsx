import { useState } from 'react'
import { ArrowUpRight, Plus, Search } from 'lucide-react'
import { Link } from 'react-router-dom'
import { EmptyState } from '@/components/empty-state'
import { PageHeader } from '@/components/page-header'
import { LifecycleBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { concernsApplication, resourceIdsOf, subjectEvidence } from '@/domain/evidence'
import { lifecycleLabels } from '@/domain/labels'
import { activeIncidents } from '@/domain/selectors'
import { counted } from '@/shared/count'
import { ApplicationForm } from './application-form'
import { EvidenceBadge } from './evidence'
import { useInventory } from './inventory-context'
import { InventoryState } from './inventory-state'

export function RealApplicationsPage() {
  const { snapshot, now } = useInventory()
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('all')
  const [editing, setEditing] = useState(false)
  const apps =
    snapshot?.applications.filter(
      (app) =>
        `${app.name} ${app.description}`.toLowerCase().includes(query.trim().toLowerCase()) &&
        (filter === 'all' || app.lifecycle === filter),
    ) ?? []
  const open = activeIncidents(snapshot?.incidents ?? [])
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory"
        title="Applications"
        description="Register applications and connect their components to infrastructure."
        action={
          <Button onClick={() => setEditing(true)}>
            <Plus className="size-4" /> New application
          </Button>
        }
      />
      <InventoryState>
        {editing && <ApplicationForm onDone={() => setEditing(false)} />}
        <div className="flex flex-wrap gap-3">
          <label className="relative w-80 max-w-full">
            <span className="sr-only">Search applications</span>
            <Search className="absolute left-3 top-2.5 size-4 text-muted-foreground" />
            <Input
              className="pl-9"
              placeholder="Find an application…"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          <select
            aria-label="Filter applications"
            className="filter-select"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          >
            <option value="all">All applications</option>
            {Object.entries(lifecycleLabels).map(([value, label]) => (
              <option value={value} key={value}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <p className="sr-only" aria-live="polite">
          {counted(apps.length, 'application')} shown
        </p>
        {apps.length ? (
          <div className="grid gap-5 min-[1060px]:grid-cols-2">
            {apps.map((app) => {
              const resourceIds = resourceIdsOf(app)
              const incidents = open.filter((item) =>
                concernsApplication(item.subject, app.id, resourceIds),
              ).length
              const evidence = subjectEvidence(
                snapshot?.observations ?? [],
                { type: 'application', id: app.id },
                now,
              )
              return (
                <Link
                  to={`/applications/${app.id}`}
                  className="panel block p-5 hover:border-muted-foreground/40"
                  key={app.id}
                  aria-label={`Open ${app.name}`}
                >
                  <div className="flex items-start justify-between gap-4">
                    <div>
                      <h2 className="text-lg font-semibold">{app.name}</h2>
                      <p className="mt-2 text-xs text-muted-foreground">
                        {app.description || 'No description'}
                      </p>
                    </div>
                    <ArrowUpRight className="size-4 text-muted-foreground" />
                  </div>
                  <div className="mt-4 flex flex-wrap gap-2">
                    <LifecycleBadge lifecycle={app.lifecycle} />
                    <EvidenceBadge state={evidence.state} compact />
                  </div>
                  <p className="mt-4 border-t pt-3 text-xs text-muted-foreground">
                    {counted(app.components.length, 'component')} ·{' '}
                    {counted(resourceIds.length, 'linked resource')}
                    {incidents > 0 && (
                      <span className="text-amber-200">
                        {' '}
                        · {incidents} open {incidents === 1 ? 'incident' : 'incidents'}
                      </span>
                    )}
                  </p>
                </Link>
              )
            })}
          </div>
        ) : (
          <EmptyState
            title={
              snapshot?.applications.length ? 'No applications found' : 'No applications registered'
            }
            description={
              snapshot?.applications.length
                ? 'Try a different search or lifecycle.'
                : 'Create an application to start your real inventory.'
            }
            action={
              !snapshot?.applications.length ? (
                <Button onClick={() => setEditing(true)}>New application</Button>
              ) : undefined
            }
          />
        )}
      </InventoryState>
    </div>
  )
}
