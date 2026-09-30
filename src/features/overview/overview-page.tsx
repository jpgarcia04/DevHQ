import { ChevronRight, Clock3, Server } from 'lucide-react'
import { Link } from 'react-router-dom'
import { PageHeader } from '@/components/page-header'
import { HealthBadge } from '@/components/status-badge'
import { ApplicationsSummary } from './applications-summary'
import { AutomationsSummary } from './automations-summary'
import { EcosystemSummary } from './ecosystem-summary'
import { events, incidents, resources } from '@/data/mock/scenario'
import { activeIncidents, needsAttention } from '@/domain/selectors'
import { IncidentList } from '@/features/incidents/incident-list'
import { ActivityTimeline } from '@/features/activity/activity-timeline'

export function OverviewPage() {
  const open = activeIncidents(incidents)
  const affected = resources.filter((resource) => needsAttention(resource.health))
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Your operations, in perspective"
        title="Overview"
        description="A clear view of what’s running. A focused view of what needs you."
        action={
          <span className="flex items-center gap-2 text-xs text-muted-foreground">
            <Clock3 className="size-3.5" aria-hidden="true" />
            Thursday, 24 September
          </span>
        }
      />
      <EcosystemSummary />
      <div className="grid gap-7 min-[1180px]:grid-cols-[minmax(0,1.6fr)_minmax(280px,1fr)]">
        <div className="space-y-7">
          <section aria-labelledby="attention-heading">
            <div className="mb-3 flex items-center justify-between">
              <h2 id="attention-heading" className="section-heading">
                Needs attention{' '}
                <span className="ml-2 rounded bg-amber-300/10 px-1.5 py-0.5 font-mono text-[10px] text-amber-200">
                  {open.length}
                </span>
              </h2>
              <Link className="quiet-link" to="/incidents">
                All incidents <ChevronRight className="size-3" />
              </Link>
            </div>
            <div className="panel overflow-hidden">
              <IncidentList items={open} />
            </div>
          </section>
          <ApplicationsSummary />
          <section aria-labelledby="infra-heading">
            <div className="mb-3 flex items-center justify-between">
              <h2 id="infra-heading" className="section-heading">
                Infrastructure watch
              </h2>
              <Link to="/infrastructure" className="quiet-link">
                View resources <ChevronRight className="size-3" />
              </Link>
            </div>
            {affected.map((resource) => (
              <Link
                key={resource.id}
                to={`/infrastructure?resource=${resource.id}`}
                className="panel flex items-start gap-3 px-4 py-4 hover:border-muted-foreground/40"
              >
                <span className="rounded-lg border p-2 text-muted-foreground">
                  <Server className="size-4" />
                </span>
                <div className="flex-1">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <h3 className="text-[13px] font-medium">{resource.name}</h3>
                    <HealthBadge health={resource.health} compact />
                  </div>
                  <p className="mt-2 text-xs leading-5 text-muted-foreground">
                    {resource.healthSummary}
                  </p>
                </div>
              </Link>
            ))}
          </section>
        </div>
        <div className="space-y-7">
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
              <p className="eyebrow mb-5">Today · CEST</p>
              <ActivityTimeline items={events.slice(0, 5)} />
            </div>
          </section>
          <AutomationsSummary />
        </div>
      </div>
    </div>
  )
}
