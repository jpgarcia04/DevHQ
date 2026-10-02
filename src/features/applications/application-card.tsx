import { ArrowRight, Box, Database, Globe, Server } from 'lucide-react'
import { Link } from 'react-router-dom'
import { AppLogo } from '@/components/app-logo'
import { HealthBadge, LifecycleBadge } from '@/components/status-badge'
import type { Application } from '@/domain/models'
import { activeIncidents, applicationIncidents } from '@/domain/selectors'
import { incidents } from '@/data/mock/scenario'
import { counted } from '@/shared/count'

const componentIcons = { backend: Server, database: Database, docker: Box, frontend: Globe }
export function ApplicationCard({ app }: { app: Application }) {
  const incidentCount = activeIncidents(applicationIncidents(incidents, app)).length
  return (
    <Link to={`/applications/${app.id}`} className="app-card" aria-label={`Open ${app.name}`}>
      <div
        className={`app-art art-${app.artwork} relative flex h-28 items-center justify-between overflow-hidden border-b border-border px-6`}
      >
        {app.thumbnailUrl ? (
          <img src={app.thumbnailUrl} alt="" className="absolute inset-0 size-full object-cover" />
        ) : (
          <>
            <div className="relative z-10">
              <AppLogo app={app} large />
            </div>
            {/* Decoration: the lifecycle is stated below, and the ID is not for people. */}
            <span
              aria-hidden="true"
              className="select-none font-mono text-[10px] uppercase tracking-[0.18em] opacity-55"
            >
              {app.lifecycle === 'production' ? 'Production system' : 'In the making'}
              <br />
              <span className="mt-1.5 block text-right text-[9px] opacity-60">{app.id}</span>
            </span>
          </>
        )}
      </div>
      <div className="p-5">
        <div className="flex items-start justify-between gap-3">
          <h2 className="text-lg font-semibold tracking-tight">{app.name}</h2>
          <ArrowRight
            className="app-card-arrow mt-1 size-4 shrink-0 text-muted-foreground transition-transform"
            aria-hidden="true"
          />
        </div>
        <p className="mt-2 min-h-10 text-xs leading-5 text-muted-foreground">{app.description}</p>
        <div className="mt-4 flex items-center gap-2">
          <LifecycleBadge lifecycle={app.lifecycle} />
          <span className="font-mono text-[10px] text-muted-foreground">
            {app.version ? `v${app.version}` : 'Pre-release'}
          </span>
        </div>
        <div className="mt-5 flex flex-wrap gap-3 border-t pt-4">
          {app.components.map((component) => {
            const Icon = componentIcons[component.kind]
            return (
              <span
                key={component.id}
                className="flex items-center gap-1.5 text-[11px] text-muted-foreground"
              >
                <Icon className="size-3" aria-hidden="true" />
                {component.name}
              </span>
            )
          })}
        </div>
      </div>
      <div className="flex items-center justify-between gap-2 border-t bg-black/10 px-5 py-3">
        <HealthBadge health={app.health} compact />
        <span className="text-[10px] text-muted-foreground">
          {incidentCount
            ? counted(incidentCount, 'linked incident')
            : app.health === 'unknown'
              ? 'Awaiting observations'
              : 'No open incidents'}
        </span>
      </div>
    </Link>
  )
}
