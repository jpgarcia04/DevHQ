import type { Incident } from '@/domain/models'
import { scenario } from '@/data/mock/scenario'
import { incidentStatusLabels } from '@/domain/labels'
import { EntityLink } from '@/components/entity-link'
import { SeverityBadge } from '@/components/status-badge'
import { relativeTime } from '@/shared/time'
import { ChevronDown } from 'lucide-react'

export function IncidentList({ items }: { items: Incident[] }) {
  if (!items.length)
    return (
      <p className="py-8 text-center text-sm text-muted-foreground">No incidents in this view.</p>
    )
  return (
    <div className="divide-y">
      {items.map((incident) => (
        <details key={incident.id} className="group px-5 py-4 open:bg-white/[0.015]">
          <summary className="flex list-none items-start gap-4 rounded-sm [&::-webkit-details-marker]:hidden">
            <span
              className={`mt-1.5 h-7 w-0.5 shrink-0 rounded-full ${incident.severity === 'critical' ? 'bg-rose-300/75' : incident.severity === 'warning' ? 'bg-amber-300/75' : 'bg-sky-300/75'}`}
            />
            <span className="flex-1">
              <span className="block text-[13px] font-medium">{incident.title}</span>
              <span className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
                <SeverityBadge severity={incident.severity} />
                <span className="text-[11px] text-muted-foreground">
                  {incidentStatusLabels[incident.status]}
                </span>
                <span className="font-mono text-[10px] text-muted-foreground">{incident.id}</span>
              </span>
            </span>
            <time
              className="pt-0.5 text-[11px] text-muted-foreground"
              dateTime={incident.occurredAt}
            >
              {relativeTime(incident.occurredAt, scenario.observedAt)}
            </time>
            <ChevronDown
              className="mt-0.5 size-4 text-muted-foreground transition-transform group-open:rotate-180"
              aria-hidden="true"
            />
          </summary>
          <div className="ml-4.5 mt-4 border-t pt-4">
            <EntityLink subject={incident.subject} className="text-xs" />
            <p className="mt-2 max-w-3xl text-[13px] leading-6 text-muted-foreground">
              {incident.description}
            </p>
          </div>
        </details>
      ))}
    </div>
  )
}
