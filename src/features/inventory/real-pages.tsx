import { useEffect, useState } from 'react'
import { ArrowLeft, ArrowUpRight, Boxes, Layers3, Pencil, Plus, Search } from 'lucide-react'
import { Link, useParams, useSearchParams } from 'react-router-dom'
import { EmptyState } from '@/components/empty-state'
import { PageHeader } from '@/components/page-header'
import { HealthBadge, LifecycleBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { lifecycleLabels } from '@/domain/labels'
import { ApplicationForm } from './application-form'
import { useInventory } from './inventory-context'
import { InventoryState } from './inventory-state'
import { ResourceForm } from './resource-form'

export function RealOverviewPage() {
  const { snapshot } = useInventory()
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory · no live observations"
        title="Overview"
        description="What needs attention starts with knowing what exists. Monitoring and incidents have not been connected."
      />
      <InventoryState>
        <div className="grid gap-5 min-[1050px]:grid-cols-3">
          <Link
            to="/applications"
            className="panel flex items-center gap-4 p-5 hover:border-muted-foreground/40"
          >
            <Layers3 className="size-5 text-primary" />
            <div>
              <p className="text-2xl font-semibold">{snapshot?.applications.length ?? 0}</p>
              <p className="text-xs text-muted-foreground">Applications</p>
            </div>
          </Link>
          <Link
            to="/infrastructure"
            className="panel flex items-center gap-4 p-5 hover:border-muted-foreground/40"
          >
            <Boxes className="size-5 text-primary" />
            <div>
              <p className="text-2xl font-semibold">{snapshot?.resources.length ?? 0}</p>
              <p className="text-xs text-muted-foreground">Resources</p>
            </div>
          </Link>
          <div className="panel p-5">
            <HealthBadge health="unknown" />
            <p className="mt-3 text-xs leading-5 text-muted-foreground">
              No checks or observations exist for real inventory yet. Lifecycle does not imply
              health.
            </p>
          </div>
        </div>
        <section className="panel p-6">
          <h2 className="section-heading">Needs attention</h2>
          <p className="mt-3 text-xs leading-6 text-muted-foreground">
            No real observation source is connected. DevHQ cannot determine whether these systems
            need attention yet.
          </p>
        </section>
      </InventoryState>
    </div>
  )
}

export function RealApplicationsPage() {
  const { snapshot } = useInventory()
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('all')
  const [editing, setEditing] = useState(false)
  const apps =
    snapshot?.applications.filter(
      (app) =>
        `${app.name} ${app.description}`.toLowerCase().includes(query.trim().toLowerCase()) &&
        (filter === 'all' || app.lifecycle === filter),
    ) ?? []
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
          {apps.length} applications shown
        </p>
        {apps.length ? (
          <div className="grid gap-5 min-[1060px]:grid-cols-2">
            {apps.map((app) => (
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
                  <HealthBadge health="unknown" compact />
                </div>
                <p className="mt-4 border-t pt-3 text-xs text-muted-foreground">
                  {app.components.length} components ·{' '}
                  {new Set(app.components.map((part) => part.resourceId).filter(Boolean)).size}{' '}
                  linked resources
                </p>
              </Link>
            ))}
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

const tabs = [
  'Overview',
  'Health',
  'Infrastructure',
  'Activity',
  'Security',
  'Automations',
] as const
export function RealApplicationDetailPage() {
  const { snapshot } = useInventory()
  const { applicationId } = useParams()
  const [params, setParams] = useSearchParams()
  const [editing, setEditing] = useState(false)
  const app = snapshot?.applications.find((item) => item.id === applicationId)
  const tab = tabs.find((item) => item.toLowerCase() === params.get('tab')) ?? 'Overview'
  return (
    <InventoryState>
      {app ? (
        <div className="page-stack">
          <Link to="/applications" className="quiet-link">
            <ArrowLeft className="size-3.5" /> All applications
          </Link>
          <header className="flex flex-wrap items-start justify-between gap-5">
            <div>
              <h1 className="text-[28px] font-semibold tracking-[-0.04em]">{app.name}</h1>
              <p className="mt-1 text-xs text-muted-foreground">{app.description}</p>
              <div className="mt-3 flex gap-3">
                <LifecycleBadge lifecycle={app.lifecycle} />
                <HealthBadge health="unknown" />
              </div>
            </div>
            <Button variant="outline" onClick={() => setEditing(true)}>
              <Pencil className="size-3.5" /> Edit application
            </Button>
          </header>
          {editing && <ApplicationForm application={app} onDone={() => setEditing(false)} />}
          <Tabs
            value={tab.toLowerCase()}
            onValueChange={(value) => setParams(value === 'overview' ? {} : { tab: value })}
            className="gap-7"
          >
            <div className="overflow-x-auto border-b pb-1">
              <TabsList variant="line" aria-label="Application sections" className="gap-4">
                {tabs.map((item) => (
                  <TabsTrigger
                    value={item.toLowerCase()}
                    key={item}
                    className="px-1 pb-3 text-xs after:bg-primary"
                  >
                    {item}
                  </TabsTrigger>
                ))}
              </TabsList>
            </div>
            <TabsContent value="overview">
              <div className="grid gap-5 min-[1100px]:grid-cols-[1.5fr_1fr]">
                <section className="panel p-5">
                  <h2 className="section-heading">Components</h2>
                  <div className="mt-4 divide-y">
                    {app.components.map((part) => (
                      <div
                        className="flex flex-wrap items-center justify-between gap-2 py-3 text-xs"
                        key={part.id}
                      >
                        <div>
                          <p className="font-medium">
                            {part.name} <span className="text-muted-foreground">· {part.kind}</span>
                          </p>
                          {part.resourceId && (
                            <Link
                              className="quiet-link mt-1"
                              to={`/infrastructure?resource=${part.resourceId}`}
                            >
                              On{' '}
                              {snapshot?.resources.find((item) => item.id === part.resourceId)
                                ?.name ?? part.resourceId}{' '}
                              <ArrowUpRight className="size-3" />
                            </Link>
                          )}
                        </div>
                        <HealthBadge health="unknown" compact />
                      </div>
                    ))}
                    {app.components.length === 0 && (
                      <p className="text-xs text-muted-foreground">No components registered.</p>
                    )}
                  </div>
                </section>
                <section className="panel p-5">
                  <h2 className="section-heading">Operational context</h2>
                  <dl className="mt-4 space-y-4 text-xs">
                    <div>
                      <dt className="text-muted-foreground">Lifecycle</dt>
                      <dd className="mt-1">{lifecycleLabels[app.lifecycle]}</dd>
                    </div>
                    <div>
                      <dt className="text-muted-foreground">Version</dt>
                      <dd className="mt-1">{app.version || 'Not recorded'}</dd>
                    </div>
                    <div>
                      <dt className="text-muted-foreground">Health</dt>
                      <dd className="mt-1">Not observed. No checks have run.</dd>
                    </div>
                  </dl>
                </section>
              </div>
            </TabsContent>
            {tabs
              .filter((item) => item !== 'Overview')
              .map((item) => (
                <TabsContent value={item.toLowerCase()} key={item}>
                  <EmptyState
                    title={`${item} has no real records yet`}
                    description={
                      item === 'Infrastructure'
                        ? 'Component to resource links are shown in Overview. More topology context is planned.'
                        : 'This area will be populated when real observations and integrations are implemented.'
                    }
                  />
                </TabsContent>
              ))}
          </Tabs>
        </div>
      ) : (
        <EmptyState
          title="Application not found"
          description="This application is not in the local inventory."
          action={
            <Button asChild variant="outline">
              <Link to="/applications">Back to applications</Link>
            </Button>
          }
        />
      )}
    </InventoryState>
  )
}

export function RealInfrastructurePage() {
  const { snapshot } = useInventory()
  const [params] = useSearchParams()
  const selectedResource = params.get('resource')
  const [editing, setEditing] = useState<string | 'new' | null>(null)
  useEffect(() => {
    if (!selectedResource) return
    const timeout = window.setTimeout(() => {
      const element = document.getElementById(`resource-${selectedResource}`)
      element?.focus({ preventScroll: true })
      element?.scrollIntoView?.({ block: 'start' })
    }, 0)
    return () => window.clearTimeout(timeout)
  }, [selectedResource, snapshot])
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory"
        title="Infrastructure"
        description="Resources shared by the components in your applications."
        action={
          <Button onClick={() => setEditing('new')}>
            <Plus className="size-4" /> New resource
          </Button>
        }
      />
      <InventoryState>
        {editing === 'new' && <ResourceForm onDone={() => setEditing(null)} />}
        {snapshot?.resources.length ? (
          <div className="space-y-5">
            {snapshot.resources.map((resource) => {
              const linked = snapshot.applications.filter((app) =>
                app.components.some((part) => part.resourceId === resource.id),
              )
              return (
                <section
                  className={`panel p-5 ${params.get('resource') === resource.id ? 'border-primary/50' : ''}`}
                  id={`resource-${resource.id}`}
                  tabIndex={-1}
                  aria-label={resource.name}
                  key={resource.id}
                >
                  <div className="flex flex-wrap items-start justify-between gap-4">
                    <div>
                      <h2 className="text-base font-semibold">{resource.name}</h2>
                      <p className="mt-1 text-xs text-muted-foreground">
                        {resource.description || resource.kind}
                      </p>
                    </div>
                    <div className="flex items-center gap-3">
                      <HealthBadge health="unknown" />
                      <Button variant="outline" onClick={() => setEditing(resource.id)}>
                        <Pencil className="size-3.5" /> Edit
                      </Button>
                    </div>
                  </div>
                  <p className="mt-4 text-xs text-muted-foreground">
                    {resource.operatingSystem || 'Operating system not recorded'} · No live checks
                  </p>
                  <div className="mt-4 border-t pt-4">
                    <h3 className="eyebrow mb-3">Applications using this resource</h3>
                    {linked.length ? (
                      <div className="flex flex-wrap gap-3">
                        {linked.map((app) => (
                          <Link className="quiet-link" to={`/applications/${app.id}`} key={app.id}>
                            {app.name} <ArrowUpRight className="size-3" />
                          </Link>
                        ))}
                      </div>
                    ) : (
                      <p className="text-xs text-muted-foreground">No linked components.</p>
                    )}
                  </div>
                  {editing === resource.id && (
                    <div className="mt-5">
                      <ResourceForm resource={resource} onDone={() => setEditing(null)} />
                    </div>
                  )}
                </section>
              )
            })}
          </div>
        ) : (
          <EmptyState
            title="No resources registered"
            description="Add a resource, then link an application component to it."
            action={<Button onClick={() => setEditing('new')}>New resource</Button>}
          />
        )}
      </InventoryState>
    </div>
  )
}

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

export function RealSettingsPage() {
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory"
        title="Settings"
        description="Your data is stored locally on this device."
      />
      <section className="panel max-w-3xl p-6">
        <h2 className="section-heading">Data source</h2>
        <p className="mt-4 text-xs leading-6 text-muted-foreground">
          Applications, components and resources are stored in SQLite in the operating system’s
          DevHQ application data directory. The demo is a separate, fixed example and is never
          imported automatically.
        </p>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          Health is not observed. No external connections, checks, event collection or automation
          execution are active.
        </p>
      </section>
    </div>
  )
}
