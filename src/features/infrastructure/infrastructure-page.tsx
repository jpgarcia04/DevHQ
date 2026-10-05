import { useEffect } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import { ArrowUpRight, Server } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { HealthBadge } from '@/components/status-badge'
import { AppLogo } from '@/components/app-logo'
import { applications, incidents, resources } from '@/data/mock/scenario'
import { activeIncidents, resourceApplications, resourceIncidents } from '@/domain/selectors'
import { counted } from '@/shared/count'
import { cn } from '@/shared/utils'
import { IncidentList } from '@/features/incidents/incident-list'

export function InfrastructurePage() {
  const [params] = useSearchParams()
  const selectedResource = params.get('resource')
  useEffect(() => {
    if (!selectedResource) return
    const timeout = window.setTimeout(() => {
      const element = document.getElementById(`resource-${selectedResource}`)
      element?.focus({ preventScroll: true })
      element?.scrollIntoView?.({ block: 'start' })
    }, 0)
    return () => window.clearTimeout(timeout)
  }, [selectedResource])
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="The systems underneath"
        title="Infrastructure"
        description="Shared resources, the applications they support, and the issues that connect them."
        action={
          <span className="text-xs text-muted-foreground">
            {counted(resources.length, 'resource')}
          </span>
        }
      />
      <div className="space-y-5">
        {resources.map((resource) => (
          <section
            key={resource.id}
            id={`resource-${resource.id}`}
            tabIndex={-1}
            className={cn(
              'panel scroll-mt-6 overflow-hidden outline-none',
              selectedResource === resource.id && 'border-primary/50',
            )}
            aria-label={resource.name}
          >
            <div className="flex flex-wrap items-start justify-between gap-4 p-6">
              <div className="flex items-start gap-4">
                <span className="rounded-xl border bg-white/3 p-3">
                  <Server className="size-5 text-muted-foreground" aria-hidden="true" />
                </span>
                <div>
                  <h2 className="text-base font-semibold">{resource.name}</h2>
                  <p className="mt-1.5 text-xs text-muted-foreground">{resource.description}</p>
                </div>
              </div>
              <HealthBadge health={resource.health} />
            </div>
            <div className="grid gap-6 px-6 pb-6 min-[1060px]:grid-cols-[1fr_1fr]">
              <div>
                <p className="eyebrow mb-3">Resource context</p>
                <dl>
                  <dt className="inline text-xs text-muted-foreground">Operating system</dt>
                  <dd className="ml-4 inline text-xs">{resource.operatingSystem}</dd>
                </dl>
                <div className="mt-4 flex flex-wrap gap-2">
                  {resource.services.map((name) => (
                    <span
                      className="rounded-md border bg-white/2 px-2.5 py-1 font-mono text-[10px] text-muted-foreground"
                      key={name}
                    >
                      {name}
                    </span>
                  ))}
                </div>
                <p className="mt-4 max-w-lg text-xs leading-6 text-muted-foreground">
                  {resource.healthSummary}
                </p>
              </div>
              <div className="min-[1060px]:border-l min-[1060px]:pl-6">
                <p className="eyebrow mb-3">Applications using this resource</p>
                <div className="space-y-3">
                  {resourceApplications(applications, resource.id).map((app) => (
                    <Link
                      to={`/applications/${app.id}`}
                      key={app.id}
                      className="flex items-center gap-3 rounded-md p-1 hover:bg-white/3"
                    >
                      <AppLogo app={app} />
                      <span className="flex-1 text-xs">{app.name}</span>
                      <ArrowUpRight className="size-3.5 text-muted-foreground" aria-hidden="true" />
                    </Link>
                  ))}
                </div>
              </div>
            </div>
            {activeIncidents(resourceIncidents(incidents, resource.id)).length > 0 && (
              <div className="border-t">
                <h3 className="eyebrow px-6 pt-5">Open resource incidents</h3>
                <IncidentList items={activeIncidents(resourceIncidents(incidents, resource.id))} />
              </div>
            )}
          </section>
        ))}
      </div>
      <p className="text-xs text-muted-foreground">
        Sample inventory. No SSH sessions, resource discovery or live checks are active.
      </p>
    </div>
  )
}
