import { AlertTriangle, CircleAlert, Info } from 'lucide-react'
import { healthLabels, lifecycleLabels, severityLabels } from '@/domain/labels'
import type { HealthStatus, IncidentSeverity, Lifecycle } from '@/domain/models'
import { cn } from '@/shared/utils'

const healthColors: Record<HealthStatus, string> = {
  healthy: 'text-emerald-300',
  degraded: 'text-amber-300',
  unhealthy: 'text-rose-300',
  unknown: 'text-muted-foreground',
}

export function HealthBadge({
  health,
  compact = false,
}: {
  health: HealthStatus
  compact?: boolean
}) {
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-2 text-xs',
        healthColors[health],
        !compact && 'rounded-md border border-current/15 bg-current/5 px-2.5 py-1',
      )}
    >
      <span className="health-dot" aria-hidden="true" />
      {healthLabels[health]}
    </span>
  )
}

export function LifecycleBadge({ lifecycle }: { lifecycle: Lifecycle }) {
  return (
    <span className="rounded border px-2 py-0.5 text-[10px] font-medium text-muted-foreground">
      {lifecycleLabels[lifecycle]}
    </span>
  )
}

export function SeverityBadge({ severity }: { severity: IncidentSeverity }) {
  const Icon = severity === 'critical' ? CircleAlert : severity === 'warning' ? AlertTriangle : Info
  return (
    <span
      className={cn('inline-flex items-center gap-1.5 text-xs', {
        'text-rose-300': severity === 'critical',
        'text-amber-300': severity === 'warning',
        'text-sky-300': severity === 'info',
      })}
    >
      <Icon className="size-3.5" aria-hidden="true" />
      {severityLabels[severity]}
    </span>
  )
}
