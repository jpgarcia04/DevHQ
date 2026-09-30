import { Box, Database, Globe, Server, ShieldCheck, ArrowUpRight } from 'lucide-react'
import { Link } from 'react-router-dom'
import { EntityLink } from '@/components/entity-link'
import type { Application } from '@/domain/models'
import { HealthBadge } from '@/components/status-badge'
import { events, incidents, scenario } from '@/data/mock/scenario'
import { activeIncidents, applicationEvents, applicationIncidents } from '@/domain/selectors'
import { relativeTime } from '@/shared/time'
import { ActivityTimeline } from '@/features/activity/activity-timeline'
import { IncidentList } from '@/features/incidents/incident-list'

const componentIcons = { backend: Server, database: Database, docker: Box, frontend: Globe }
export function ApplicationOverview({ app }: { app: Application }) {
  const open = activeIncidents(applicationIncidents(incidents, app))
  const activity = applicationEvents(events, app)
  const facts = [
    ['Current version', app.version ?? 'Not released'],
    [
      'Last verified deployment',
      app.lastVerifiedDeploymentAt
        ? relativeTime(app.lastVerifiedDeploymentAt, scenario.observedAt)
        : 'No verified deployment recorded',
    ],
    [
      'Last backup',
      app.lastBackupAt ? relativeTime(app.lastBackupAt, scenario.observedAt) : 'No backup recorded',
    ],
    ['Open incidents', `${open.length} linked ${open.length === 1 ? 'incident' : 'incidents'}`],
  ]
  return (
    <div className="grid gap-6 min-[1180px]:grid-cols-[minmax(0,1.5fr)_minmax(260px,1fr)]">
      <div className="space-y-6">
        <section className="panel p-5">
          <div className="flex items-center justify-between gap-4">
            <h2 className="section-heading">Application health</h2>
            <HealthBadge health={app.health} />
          </div>
          <p className="mb-6 mt-3 text-xs leading-5 text-muted-foreground">{app.healthSummary}</p>
          <div className="divide-y border-t">
            {app.components.map((component) => {
              const Icon = componentIcons[component.kind]
              return (
                <div
                  key={component.id}
                  className="flex items-center justify-between py-4 last:pb-0"
                >
                  <span className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                    <Icon className="size-4 text-muted-foreground" aria-hidden="true" />
                    {component.name}
                    {component.resourceId && (
                      <span className="text-[11px] text-muted-foreground">
                        on{' '}
                        <EntityLink
                          subject={{ type: 'resource', id: component.resourceId }}
                          className="underline-offset-2"
                        />
                      </span>
                    )}
                  </span>
                  <HealthBadge health={component.health} compact />
                </div>
              )
            })}
          </div>
        </section>
        <section className="panel p-5">
          <h2 className="section-heading mb-5">Recent activity</h2>
          <ActivityTimeline items={activity.slice(0, 4)} detailed />
        </section>
      </div>
      <div className="space-y-6">
        <section className="panel p-5">
          <h2 className="section-heading mb-5">Operational context</h2>
          <dl className="space-y-5">
            {facts.map(([label, value]) => (
              <div key={label}>
                <dt className="text-xs text-muted-foreground">{label}</dt>
                <dd className="mt-1.5 text-[13px] font-medium">{value}</dd>
              </div>
            ))}
          </dl>
        </section>
        {open.length > 0 ? (
          <section>
            <h2 className="section-heading mb-3">Linked incidents</h2>
            <div className="panel">
              <IncidentList items={open} />
            </div>
          </section>
        ) : (
          <div className="flex gap-3 rounded-xl border border-dashed p-5">
            <ShieldCheck className="size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
            <div>
              <h2 className="text-xs font-medium">No open incidents</h2>
              <p className="mt-2 text-xs leading-5 text-muted-foreground">
                No issues recorded for this application or its linked infrastructure in the sample.
              </p>
            </div>
          </div>
        )}
        <Link to="/activity" className="quiet-link">
          Explore the global timeline <ArrowUpRight className="size-3.5" />
        </Link>
      </div>
    </div>
  )
}
