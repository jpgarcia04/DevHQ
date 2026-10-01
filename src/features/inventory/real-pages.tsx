import { useEffect, useState } from 'react'
import {
  ArrowLeft,
  ArrowUpRight,
  Boxes,
  ChevronRight,
  Layers3,
  Pencil,
  Plus,
  Search,
  Server,
  ShieldCheck,
} from 'lucide-react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { EmptyState } from '@/components/empty-state'
import { PageHeader } from '@/components/page-header'
import { LifecycleBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  attentionItems,
  concernsApplication,
  evidenceCoverage,
  subjectEvidence,
  subjectKey,
  subjectsOf,
  workspaceStatus,
  type Evidence,
  type EvidenceState,
} from '@/domain/evidence'
import { lifecycleLabels } from '@/domain/labels'
import type { EntityRef } from '@/domain/models'
import { activeIncidents } from '@/domain/selectors'
import { relativeTime } from '@/shared/time'
import { cn } from '@/shared/utils'
import { ApplicationForm } from './application-form'
import { ApplicationInfrastructure } from './application-infrastructure'
import {
  AutomatedWindowNote,
  EvidenceBadge,
  EvidenceDetail,
  ObservationHistory,
  SourceTag,
  SubjectEvidence,
  SubjectLink,
} from './evidence'
import { automatedSince } from './history'
import { HttpCheckPanel } from './http-check'
import type { InventoryApplication } from './inventory-api'
import { useInventory } from './inventory-context'
import { InventoryState } from './inventory-state'
import { RecordActions } from './record-forms'
import { AttentionList, EventRecordList, IncidentRecordList } from './record-lists'
import { ResourceForm } from './resource-form'
import { SupervisionOverview } from './supervision-overview'

const resourceIdsOf = (app: InventoryApplication) => [
  ...new Set(app.components.flatMap((part) => (part.resourceId ? [part.resourceId] : []))),
]

// Problems first, then missing or expired evidence, then current healthy results.
const coverageOrder: EvidenceState[] = ['unhealthy', 'degraded', 'stale', 'unobserved', 'healthy']

function coverageNote(evidence: Evidence, now: number) {
  if (!evidence.latest) return 'Never observed'
  if (evidence.state === 'stale')
    return `Expired ${relativeTime(evidence.latest.validUntil, now).toLowerCase()}`
  return `Observed ${relativeTime(evidence.latest.observedAt, now).toLowerCase()}`
}

export function RealOverviewPage() {
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory · recorded evidence"
        title="Overview"
        description="What needs your attention, judged only from evidence recorded in DevHQ."
      />
      <InventoryState>
        <OverviewContent />
      </InventoryState>
    </div>
  )
}

function OverviewContent() {
  const { snapshot, now } = useInventory()
  if (!snapshot) return null
  const subjects = subjectsOf(snapshot)
  const attention = attentionItems(subjects, snapshot.observations, snapshot.incidents, now)
  const coverage = evidenceCoverage(subjects, snapshot.observations, now)
  const status = workspaceStatus(attention, coverage)
  const evidence = subjects
    .map((subject) => ({ subject, evidence: subjectEvidence(snapshot.observations, subject, now) }))
    .sort(
      (a, b) => coverageOrder.indexOf(a.evidence.state) - coverageOrder.indexOf(b.evidence.state),
    )
  const summary = {
    empty: [
      'Nothing is registered yet.',
      'Add applications and resources, then record what you observe.',
    ],
    attention: [
      `${attention.length} ${attention.length === 1 ? 'item needs' : 'items need'} your attention.`,
      'Open incidents, observed problems and expired evidence are listed below.',
    ],
    unobserved: [
      'No recorded problems, but coverage is incomplete.',
      `${coverage.unobserved} of ${subjects.length} systems have never been observed. DevHQ does not assume they are healthy.`,
    ],
    covered: [
      'Every system has current healthy evidence.',
      'Based on manual entries and HTTP checks that are still within their validity.',
    ],
  }[status]
  return (
    <>
      <section
        aria-label="Ecosystem status"
        className={cn(
          'panel flex flex-wrap items-center justify-between gap-6 border-l-2 px-6 py-5',
          status === 'attention'
            ? 'border-l-amber-300/70'
            : status === 'covered'
              ? 'border-l-emerald-300/60'
              : 'border-l-muted-foreground/50',
        )}
      >
        <div className="min-w-0">
          <p className="eyebrow mb-2.5">Ecosystem status</p>
          <h2 className="text-lg font-medium tracking-tight">{summary[0]}</h2>
          <p className="mt-1.5 text-xs text-muted-foreground">{summary[1]}</p>
        </div>
        <dl className="flex gap-6 text-xs">
          {(
            [
              ['Current evidence', coverage.current],
              ['Expired', coverage.stale],
              ['Never observed', coverage.unobserved],
            ] as const
          ).map(([label, count]) => (
            <div key={label}>
              <dt className="text-[11px] text-muted-foreground">{label}</dt>
              <dd className="mt-1 text-lg font-semibold">{count}</dd>
            </div>
          ))}
        </dl>
      </section>
      <div className="grid gap-7 min-[1180px]:grid-cols-[minmax(0,1.6fr)_minmax(280px,1fr)]">
        <div className="space-y-7">
          <section aria-labelledby="attention-heading">
            <div className="mb-3 flex items-center justify-between">
              <h2 id="attention-heading" className="section-heading">
                Needs attention{' '}
                <span className="ml-2 rounded bg-amber-300/10 px-1.5 py-0.5 font-mono text-[10px] text-amber-200">
                  {attention.length}
                </span>
              </h2>
              <Link className="quiet-link" to="/incidents">
                All incidents <ChevronRight className="size-3" />
              </Link>
            </div>
            <div className="panel overflow-hidden">
              {attention.length ? (
                <AttentionList items={attention} />
              ) : (
                <p className="px-5 py-8 text-center text-sm text-muted-foreground">
                  No open incidents, observed problems or expired evidence.
                  {coverage.unobserved > 0 &&
                    ' Systems that were never observed are not included, so this is not a clean bill of health.'}
                </p>
              )}
            </div>
          </section>
          <section aria-labelledby="coverage-heading">
            <h2 id="coverage-heading" className="section-heading mb-3">
              Evidence coverage
            </h2>
            {evidence.length ? (
              <ul className="panel divide-y">
                {evidence.map((item) => (
                  <li
                    key={subjectKey(item.subject)}
                    className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3"
                  >
                    {item.subject.type === 'application' ? (
                      <Layers3 className="size-4 text-muted-foreground" aria-hidden="true" />
                    ) : (
                      <Server className="size-4 text-muted-foreground" aria-hidden="true" />
                    )}
                    <SubjectLink
                      subject={item.subject}
                      className="min-w-0 flex-1 truncate text-[13px] text-foreground"
                    />
                    <span className="text-[11px] text-muted-foreground">
                      {coverageNote(item.evidence, now)}
                    </span>
                    {item.evidence.latest && <SourceTag source={item.evidence.latest.source} />}
                    <EvidenceBadge state={item.evidence.state} compact />
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-xs text-muted-foreground">
                No applications or resources are registered.
              </p>
            )}
          </section>
        </div>
        <div className="space-y-7">
          <SupervisionOverview />
          <section aria-labelledby="activity-heading">
            <div className="mb-3 flex items-center justify-between">
              <h2 id="activity-heading" className="section-heading">
                Recent activity
              </h2>
              <Link className="quiet-link" to="/activity">
                Timeline <ChevronRight className="size-3" />
              </Link>
            </div>
            <div className="panel p-5">
              {snapshot.events.length ? (
                <EventRecordList items={snapshot.events} foldRepeats maxRows={5} detailed={false} />
              ) : (
                <p className="text-xs text-muted-foreground">No activity recorded yet.</p>
              )}
            </div>
          </section>
          <div className="grid grid-cols-2 gap-4">
            <Link
              to="/applications"
              className="panel flex items-center gap-3 p-4 hover:border-muted-foreground/40"
            >
              <Layers3 className="size-4 text-primary" aria-hidden="true" />
              <span>
                <span className="block text-xl font-semibold">{snapshot.applications.length}</span>
                <span className="text-xs text-muted-foreground">Applications</span>
              </span>
            </Link>
            <Link
              to="/infrastructure"
              className="panel flex items-center gap-3 p-4 hover:border-muted-foreground/40"
            >
              <Boxes className="size-4 text-primary" aria-hidden="true" />
              <span>
                <span className="block text-xl font-semibold">{snapshot.resources.length}</span>
                <span className="text-xs text-muted-foreground">Resources</span>
              </span>
            </Link>
          </div>
        </div>
      </div>
    </>
  )
}

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
          {apps.length} applications shown
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
                    {app.components.length} components · {resourceIds.length} linked resources
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

const tabs = [
  'Overview',
  'Health',
  'Infrastructure',
  'Activity',
  'Security',
  'Automations',
] as const
const plannedTabs: Partial<Record<(typeof tabs)[number], string>> = {
  Security: 'This area will be populated when security checks and integrations are implemented.',
  Automations: 'This area will be populated when automations with verified results exist.',
}

export function RealApplicationDetailPage() {
  const { snapshot } = useInventory()
  const { applicationId } = useParams()
  const app = snapshot?.applications.find((item) => item.id === applicationId)
  return (
    <InventoryState>
      {app ? (
        <ApplicationDetail app={app} />
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

function ApplicationDetail({ app }: { app: InventoryApplication }) {
  const { snapshot, now } = useInventory()
  const [params, setParams] = useSearchParams()
  const navigate = useNavigate()
  const [editing, setEditing] = useState(false)
  const tab = tabs.find((item) => item.toLowerCase() === params.get('tab')) ?? 'Overview'
  const subject: EntityRef = { type: 'application', id: app.id }
  const resourceIds = resourceIdsOf(app)
  const observations = snapshot?.observations ?? []
  const evidence = subjectEvidence(observations, subject, now)
  const resourceState = (id: string) =>
    subjectEvidence(observations, { type: 'resource', id }, now).state
  const incidents = activeIncidents(snapshot?.incidents ?? []).filter((item) =>
    concernsApplication(item.subject, app.id, resourceIds),
  )
  const events = (snapshot?.events ?? []).filter((item) =>
    concernsApplication(item.subject, app.id, resourceIds),
  )
  const since = snapshot
    ? automatedSince(snapshot.automatedHistory, [
        subject,
        ...resourceIds.map((id) => ({ type: 'resource' as const, id })),
      ])
    : null
  return (
    <div className="page-stack">
      <Link to="/applications" className="quiet-link">
        <ArrowLeft className="size-3.5" /> All applications
      </Link>
      <header className="flex flex-wrap items-start justify-between gap-5">
        <div>
          <h1 className="text-[28px] font-semibold tracking-[-0.04em]">{app.name}</h1>
          <p className="mt-1 text-xs text-muted-foreground">{app.description}</p>
          <div className="mt-3 flex flex-wrap gap-3">
            <LifecycleBadge lifecycle={app.lifecycle} />
            <EvidenceBadge state={evidence.state} />
          </div>
        </div>
        <Button variant="outline" onClick={() => setEditing(true)}>
          <Pencil className="size-3.5" /> Edit application
        </Button>
      </header>
      {editing && (
        <ApplicationForm
          application={app}
          onDone={() => setEditing(false)}
          onDeleted={() => navigate('/applications')}
        />
      )}
      <RecordActions subject={subject} name={app.name} />
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
            <div className="space-y-5">
              <section className="panel p-5">
                <h2 className="section-heading mb-4">Current evidence</h2>
                <SubjectEvidence subject={subject} />
                <div className="mt-5 border-t pt-4">
                  <HttpCheckPanel subject={subject} name={app.name} />
                </div>
              </section>
              <section className="panel p-5">
                <h2 className="section-heading">Components</h2>
                <p className="mt-1.5 text-xs text-muted-foreground">
                  Evidence is recorded per application and resource. A linked resource shows its own
                  state.
                </p>
                <div className="mt-4 divide-y">
                  {app.components.map((part) => (
                    <div className="py-3 text-xs" key={part.id}>
                      <p className="font-medium">
                        {part.name} <span className="text-muted-foreground">· {part.kind}</span>
                      </p>
                      {part.resourceId ? (
                        <p className="mt-1 flex flex-wrap items-center gap-2">
                          <Link
                            className="quiet-link"
                            to={`/infrastructure?resource=${part.resourceId}`}
                          >
                            On{' '}
                            {snapshot?.resources.find((item) => item.id === part.resourceId)
                              ?.name ?? part.resourceId}{' '}
                            <ArrowUpRight className="size-3" />
                          </Link>
                          <EvidenceBadge state={resourceState(part.resourceId)} compact />
                        </p>
                      ) : (
                        <p className="mt-1 text-[11px] text-muted-foreground">
                          Not linked to a resource
                        </p>
                      )}
                    </div>
                  ))}
                  {app.components.length === 0 && (
                    <p className="text-xs text-muted-foreground">No components registered.</p>
                  )}
                </div>
              </section>
            </div>
            <div className="space-y-5">
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
                    <dt className="text-muted-foreground">Latest observation</dt>
                    <dd className="mt-1 flex flex-wrap items-center gap-2">
                      {evidence.latest ? (
                        <>
                          {relativeTime(evidence.latest.observedAt, now)}
                          <SourceTag source={evidence.latest.source} />
                        </>
                      ) : (
                        'None recorded'
                      )}
                    </dd>
                  </div>
                  <div>
                    <dt className="text-muted-foreground">Open incidents</dt>
                    <dd className="mt-1">
                      {incidents.length} on this application or its linked resources
                    </dd>
                  </div>
                </dl>
              </section>
              {incidents.length ? (
                <section aria-labelledby="app-incidents-heading">
                  <h2 id="app-incidents-heading" className="section-heading mb-3">
                    Open incidents
                  </h2>
                  <div className="panel overflow-hidden">
                    <IncidentRecordList items={incidents} />
                  </div>
                </section>
              ) : (
                <div className="flex gap-3 rounded-xl border border-dashed p-5">
                  <ShieldCheck
                    className="size-5 shrink-0 text-muted-foreground"
                    aria-hidden="true"
                  />
                  <div>
                    <h2 className="text-xs font-medium">No open incidents recorded</h2>
                    <p className="mt-2 text-xs leading-5 text-muted-foreground">
                      Nothing is open for this application or its linked resources. That is not a
                      health check.
                    </p>
                  </div>
                </div>
              )}
            </div>
          </div>
        </TabsContent>
        <TabsContent value="health">
          <div className="grid gap-5 min-[1100px]:grid-cols-[1.5fr_1fr]">
            <section className="panel p-5">
              <h2 className="section-heading mb-4">Observation history</h2>
              <ObservationHistory subject={subject} />
            </section>
            <section className="panel p-5">
              <h2 className="section-heading">Linked resources</h2>
              {resourceIds.length ? (
                <ul className="mt-4 space-y-3">
                  {resourceIds.map((id) => (
                    <li
                      key={id}
                      className="flex flex-wrap items-center justify-between gap-2 text-xs"
                    >
                      <SubjectLink subject={{ type: 'resource', id }} />
                      <EvidenceBadge state={resourceState(id)} compact />
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="mt-3 text-xs text-muted-foreground">
                  No components are linked to a resource.
                </p>
              )}
            </section>
          </div>
        </TabsContent>
        <TabsContent value="infrastructure">
          <ApplicationInfrastructure app={app} />
        </TabsContent>
        <TabsContent value="activity">
          <section className="panel p-5">
            <h2 className="section-heading mb-5">{app.name} and its resources</h2>
            {since && snapshot && (
              <div className="mb-5">
                <AutomatedWindowNote since={since} limit={snapshot.automatedHistory.limit} />
              </div>
            )}
            {events.length ? (
              <EventRecordList foldRepeats items={events} automatedSince={since} />
            ) : (
              <p className="text-xs text-muted-foreground">
                No activity recorded yet. Use Log event above to add one.
              </p>
            )}
          </section>
        </TabsContent>
        {tabs
          .filter((item) => item in plannedTabs)
          .map((item) => (
            <TabsContent value={item.toLowerCase()} key={item}>
              <EmptyState
                title={`${item} has no real records yet`}
                description={plannedTabs[item]!}
              />
            </TabsContent>
          ))}
      </Tabs>
    </div>
  )
}

export function RealInfrastructurePage() {
  const { snapshot, now } = useInventory()
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
              const subject: EntityRef = { type: 'resource', id: resource.id }
              const evidence = subjectEvidence(snapshot.observations, subject, now)
              const incidents = activeIncidents(snapshot.incidents).filter(
                (item) => item.subject.type === 'resource' && item.subject.id === resource.id,
              )
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
                        {resource.description || resource.kind} ·{' '}
                        {resource.operatingSystem || 'Operating system not recorded'}
                      </p>
                    </div>
                    <div className="flex items-center gap-3">
                      <EvidenceBadge state={evidence.state} />
                      <Button variant="outline" onClick={() => setEditing(resource.id)}>
                        <Pencil className="size-3.5" /> Edit
                      </Button>
                    </div>
                  </div>
                  <EvidenceDetail evidence={evidence} now={now} />
                  <div className="mt-4 border-t pt-4">
                    <HttpCheckPanel subject={subject} name={resource.name} />
                  </div>
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
                  {incidents.length > 0 && (
                    <div className="-mx-5 mt-4 border-t">
                      <h3 className="eyebrow px-5 pt-4">Open resource incidents</h3>
                      <IncidentRecordList items={incidents} />
                    </div>
                  )}
                  <div className="mt-4 border-t pt-4">
                    <RecordActions subject={subject} name={resource.name} />
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
          Applications, components, resources, observations, incidents and events are stored in
          SQLite in the operating system’s DevHQ application data directory. The demo is a separate,
          fixed example and is never imported automatically.
        </p>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          Observations come from two sources, always labelled: manual entries you type, and HTTP
          checks that DevHQ runs when you press Run check or, if you turn supervision on, on a
          schedule while DevHQ is open. There is no background service, retry, event collection or
          automation execution. Incidents and events are manual entries. An observation vouches for
          a result only until its validity ends; after that it is shown as expired, whatever its
          source.
        </p>
      </section>
      <section className="panel max-w-3xl p-6">
        <h2 className="section-heading">HTTP checks</h2>
        <p className="mt-4 text-xs leading-6 text-muted-foreground">
          Each application or resource can have one check: a single GET sent by DevHQ’s native
          process, never by this window, straight to the URL without a proxy. Certificates are
          verified by the operating system, at most 5 redirects are followed and recorded, and the
          whole request gives up after at most 10 seconds. No credentials, custom headers or body
          are sent. Only the status, timing, redirects and error are stored, never the response body
          or headers.
        </p>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          If the checked system fails (DNS, refused connection, TLS, timeout or an unexpected
          status), that is evidence and is recorded as Unhealthy. If DevHQ fails (invalid or missing
          configuration, database), you see an error and nothing is recorded.
        </p>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          Supervision runs a check every 5 to 60 minutes, only while this window is open: closing
          DevHQ stops it, and nothing runs in the background or with the app closed. A check never
          runs twice at once and failures are not retried; the next run comes one interval after the
          last one ended. When DevHQ opens again, an overdue check runs once, not once per missed
          interval. Each result must stay valid for longer than the interval, so supervised evidence
          does not lapse between runs. Pausing lets a run in progress finish; removing a check
          cancels it and records nothing.
        </p>
      </section>
    </div>
  )
}
