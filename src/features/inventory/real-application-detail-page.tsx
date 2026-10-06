import { useState } from 'react'
import { ArrowLeft, ArrowUpRight, Pencil, ShieldCheck } from 'lucide-react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { EmptyState } from '@/components/empty-state'
import { PageHeader } from '@/components/page-header'
import { LifecycleBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { concernsApplication, resourceIdsOf, subjectEvidence } from '@/domain/evidence'
import { lifecycleLabels } from '@/domain/labels'
import type { EntityRef } from '@/domain/models'
import { activeIncidents } from '@/domain/selectors'
import { relativeTime } from '@/shared/time'
import { ApplicationForm } from './application-form'
import { ApplicationInfrastructure } from './application-infrastructure'
import {
  AutomatedWindowNote,
  EvidenceBadge,
  ObservationHistory,
  OlderResults,
  SourceTag,
  SubjectEvidence,
  SubjectLink,
} from './evidence'
import { HttpCheckPanel } from './http-check'
import type { InventoryApplication } from './inventory-api'
import { useInventory } from './inventory-context'
import { InventoryState } from './inventory-state'
import { RecordActions } from './record-forms'
import { EventRecordList, IncidentRecordList } from './record-lists'
import { useTimeline } from './timeline'

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

function AllApplications() {
  return (
    <Link to="/applications" className="quiet-link">
      <ArrowLeft className="size-3.5" /> All applications
    </Link>
  )
}

export function RealApplicationDetailPage() {
  const { snapshot } = useInventory()
  const { applicationId } = useParams()
  // Until the inventory loads, or when it cannot be read (which clears the snapshot), which
  // application this is stays unknown. The page is still titled, with the way back.
  if (!snapshot)
    return (
      <div className="page-stack">
        <AllApplications />
        <PageHeader
          eyebrow="Local inventory"
          title="Application"
          description="Its evidence, infrastructure and activity, as recorded in DevHQ."
        />
        <InventoryState>{null}</InventoryState>
      </div>
    )
  const app = snapshot.applications.find((item) => item.id === applicationId)
  if (!app)
    return (
      <EmptyState
        page
        title="Application not found"
        description="This application is not in the local inventory."
        action={
          <Button asChild variant="outline">
            <Link to="/applications">Back to applications</Link>
          </Button>
        }
      />
    )
  return <ApplicationDetail app={app} />
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
  // Its own records and those of every resource it runs on, older automated ones on demand.
  const timeline = useTimeline(`${app.id}:${resourceIds.join(',')}`, [
    subject,
    ...resourceIds.map((id) => ({ type: 'resource' as const, id })),
  ])
  return (
    <div className="page-stack">
      <AllApplications />
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
                          <SourceTag record={evidence.latest} />
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
            {timeline.since && (
              <div className="mb-5">
                <AutomatedWindowNote since={timeline.since} limit={timeline.limit} />
              </div>
            )}
            {timeline.events.length ? (
              <EventRecordList
                foldRepeats
                items={timeline.events}
                automatedSince={timeline.since}
              />
            ) : (
              <p className="text-xs text-muted-foreground">
                No activity recorded yet. Use Log event above to add one.
              </p>
            )}
            <div className="mt-5 space-y-3 empty:hidden">
              <OlderResults
                limit={timeline.limit}
                offered={timeline.since !== null}
                loading={timeline.loading}
                error={timeline.error}
                complete={timeline.complete}
                scope="these systems"
                onLoad={() => void timeline.loadOlder()}
              />
            </div>
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
