import {
  ArrowUpRight,
  Check,
  Database,
  Hammer,
  HeartPulse,
  ShieldCheck,
  Workflow,
} from 'lucide-react'
import type { ActivityEvent } from '@/domain/models'
import { scenario } from '@/data/mock/scenario'
import { EntityLink } from '@/components/entity-link'
import { formatDate, formatTime } from '@/shared/time'
import { cn } from '@/shared/utils'

const icons = {
  backup: Database,
  build: Hammer,
  health: HeartPulse,
  deployment: ArrowUpRight,
  security: ShieldCheck,
  automation: Workflow,
}
export function ActivityTimeline({
  items,
  detailed = false,
}: {
  items: ActivityEvent[]
  detailed?: boolean
}) {
  if (items.length === 0)
    return (
      <p className="py-8 text-center text-sm text-muted-foreground">
        No activity matches this view.
      </p>
    )
  return (
    <ol className="divide-y divide-border/65">
      {items.map((event) => {
        const Icon = icons[event.kind]
        return (
          <li key={event.id} className="flex gap-3 py-4 first:pt-1 last:pb-1">
            <span
              className={cn(
                'mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-lg border',
                event.outcome === 'failure'
                  ? 'border-rose-400/20 text-rose-300'
                  : event.outcome === 'warning'
                    ? 'border-amber-400/20 text-amber-300'
                    : 'text-muted-foreground',
              )}
            >
              <Icon className="size-3.5" aria-hidden="true" />
              <span className="sr-only">{event.outcome}</span>
            </span>
            <div className="min-w-0 flex-1">
              <div className="flex items-start justify-between gap-3">
                <p className="text-[13px] font-medium">{event.title}</p>
                <time
                  dateTime={event.occurredAt}
                  className="shrink-0 font-mono text-[10px] text-muted-foreground"
                >
                  {formatDate(event.occurredAt, scenario.timeZone) !==
                    formatDate(scenario.observedAt, scenario.timeZone) &&
                    `${formatDate(event.occurredAt, scenario.timeZone)} · `}
                  {formatTime(event.occurredAt, scenario.timeZone)}
                </time>
              </div>
              <EntityLink subject={event.subject} className="mt-1 inline-block text-xs" />
              {detailed && (
                <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
                  {event.description}
                </p>
              )}
            </div>
            {detailed && event.outcome === 'success' && (
              <Check aria-hidden="true" className="mt-1 size-3.5 text-emerald-300" />
            )}
          </li>
        )
      })}
    </ol>
  )
}
