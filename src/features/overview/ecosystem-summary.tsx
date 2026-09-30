import { ArrowRight } from 'lucide-react'
import { Link } from 'react-router-dom'
import { HealthBadge } from '@/components/status-badge'
import { applications, resources, incidents } from '@/data/mock/scenario'
import { activeIncidents, ecosystemHealth, needsAttention } from '@/domain/selectors'
import { cn } from '@/shared/utils'
export function EcosystemSummary() {
  const open = activeIncidents(incidents)
  const attention = applications.filter((app) => needsAttention(app.health))
  const affected = resources.filter((resource) => needsAttention(resource.health))
  const healthy = applications.filter((app) => app.health === 'healthy').length
  const unknown = applications.filter((app) => app.health === 'unknown').length
  const health = ecosystemHealth(applications, resources)
  const attentionRequired = open.length > 0 || needsAttention(health)
  return (
    <section
      aria-label="Ecosystem health"
      className={cn(
        'panel flex flex-wrap items-center justify-between gap-6 border-l-2 px-6 py-5',
        attentionRequired ? 'border-l-amber-300/70' : 'border-l-muted-foreground/50',
      )}
    >
      <div>
        <div className="mb-2.5 flex items-center gap-2">
          <span
            className={cn(
              'health-dot',
              attentionRequired ? 'text-amber-300' : 'text-muted-foreground',
            )}
          />
          <p className="eyebrow">Ecosystem status</p>
        </div>
        <h2 className="text-lg font-medium tracking-tight">
          {attentionRequired
            ? 'A few things need your attention.'
            : health === 'unknown'
              ? 'Some systems are not observed yet.'
              : 'Your ecosystem is running smoothly.'}
        </h2>
        <p className="mt-1.5 text-xs text-muted-foreground">
          {attention.length} {attention.length === 1 ? 'application' : 'applications'} and{' '}
          {affected.length} shared {affected.length === 1 ? 'resource' : 'resources'} need a closer
          look.
        </p>
      </div>
      <div className="flex items-center gap-6">
        <div className="border-r pr-6">
          <HealthBadge health={health} compact />
          <p className="mt-2 text-[11px] text-muted-foreground">
            {healthy} healthy apps · {unknown} not observed
          </p>
        </div>
        <Link
          to="/incidents"
          className="flex size-9 items-center justify-center rounded-lg border bg-white/3 text-muted-foreground hover:text-foreground"
          aria-label="Review active incidents"
        >
          <ArrowRight className="size-4" />
        </Link>
      </div>
    </section>
  )
}
