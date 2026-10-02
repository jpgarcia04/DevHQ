import { Fragment, useState } from 'react'
import {
  ArrowUpRight,
  ChevronDown,
  CircleAlert,
  Database,
  Eye,
  NotebookPen,
  Wrench,
  type LucideIcon,
} from 'lucide-react'
import { Link } from 'react-router-dom'
import { SeverityBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { subjectKey, type AttentionItem, type ObservedEvidence } from '@/domain/evidence'
import { eventKindLabels, eventOutcomeLabels, incidentStatusLabels } from '@/domain/labels'
import type {
  IncidentSeverity,
  IncidentStatus,
  RecordedEvent,
  RecordedEventKind,
  RecordedIncident,
} from '@/domain/models'
import { formatDateTime, formatTime, relativeTime } from '@/shared/time'
import { cn } from '@/shared/utils'
import { EvidenceBadge, EvidenceDetail, SourceTag, SubjectLink, WindowBoundary } from './evidence'
import { beforeWindow, timelineRows, withinWindow, type TimelineRow } from './history'
import { inventoryError } from './inventory-api'
import { useInventory } from './inventory-context'
import { textLimits } from './limits'

const severityBar: Record<IncidentSeverity, string> = {
  critical: 'bg-rose-300/75',
  warning: 'bg-amber-300/75',
  info: 'bg-sky-300/75',
}

const nextStatuses: Record<IncidentStatus, { status: IncidentStatus; label: string }[]> = {
  open: [
    { status: 'investigating', label: 'Start investigating' },
    { status: 'resolved', label: 'Resolve' },
  ],
  investigating: [{ status: 'resolved', label: 'Resolve' }],
  resolved: [{ status: 'open', label: 'Reopen' }],
}

function IncidentStatusControl({ incident }: { incident: RecordedIncident }) {
  const { updateIncidentStatus } = useInventory()
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const change = async (status: IncidentStatus) => {
    setBusy(true)
    setError(null)
    try {
      await updateIncidentStatus({ id: incident.id, status, note })
      setNote('')
    } catch (cause) {
      setError(inventoryError(cause))
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="flex flex-wrap items-end gap-2">
      <label className="min-w-52 flex-1 grid gap-1.5 text-xs">
        Status note (optional)
        <Input
          maxLength={textLimits.summary}
          value={note}
          onChange={(event) => setNote(event.target.value)}
        />
      </label>
      {nextStatuses[incident.status].map(({ status, label }) => (
        <Button key={status} variant="outline" disabled={busy} onClick={() => void change(status)}>
          {label}
        </Button>
      ))}
      {error && (
        <p role="alert" className="w-full text-xs text-rose-300">
          {error}
        </p>
      )}
    </div>
  )
}

export function IncidentRecordList({
  items,
  expanded,
}: {
  items: RecordedIncident[]
  expanded?: string | null
}) {
  const { snapshot, now } = useInventory()
  return (
    <div className="divide-y">
      {items.map((incident) => {
        const history = (snapshot?.events ?? [])
          .filter((event) => event.incidentId === incident.id)
          .reverse()
        return (
          <details
            key={incident.id}
            open={incident.id === expanded || undefined}
            className="group px-5 py-4 open:bg-white/[0.015]"
          >
            <summary className="flex list-none items-start gap-4 rounded-sm [&::-webkit-details-marker]:hidden">
              <span
                className={cn(
                  'mt-1.5 h-7 w-0.5 shrink-0 rounded-full',
                  severityBar[incident.severity],
                )}
              />
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] font-medium">{incident.title}</span>
                <span className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
                  <SeverityBadge severity={incident.severity} />
                  <span className="text-[11px] text-muted-foreground">
                    {incidentStatusLabels[incident.status]}
                  </span>
                  <SourceTag source={incident.source} />
                </span>
              </span>
              <time
                className="pt-0.5 text-[11px] text-muted-foreground"
                dateTime={incident.openedAt}
              >
                {relativeTime(incident.openedAt, now)}
              </time>
              <ChevronDown
                className="mt-0.5 size-4 text-muted-foreground transition-transform group-open:rotate-180"
                aria-hidden="true"
              />
            </summary>
            <div className="ml-4.5 mt-4 space-y-4 border-t pt-4">
              <p className="text-xs text-muted-foreground">
                Affects{' '}
                <SubjectLink
                  subject={incident.subject}
                  className="text-foreground underline decoration-muted-foreground/60 underline-offset-2"
                />{' '}
                · opened{' '}
                <time dateTime={incident.openedAt}>{formatDateTime(incident.openedAt)}</time>
              </p>
              {incident.description && (
                <p className="max-w-3xl text-[13px] leading-6 text-muted-foreground">
                  {incident.description}
                </p>
              )}
              {history.length > 0 && (
                <ol className="space-y-2 border-l pl-4" aria-label="Incident history">
                  {history.map((event) => (
                    <li key={event.id} className="text-xs text-muted-foreground">
                      <time className="font-mono text-[10px]" dateTime={event.occurredAt}>
                        {formatDateTime(event.occurredAt)}
                      </time>{' '}
                      · <span className="text-foreground">{event.title}</span>
                      {event.detail && ` — ${event.detail}`}
                    </li>
                  ))}
                </ol>
              )}
              <IncidentStatusControl incident={incident} />
            </div>
          </details>
        )
      })}
    </div>
  )
}

const kindIcons: Record<RecordedEventKind, LucideIcon> = {
  observation: Eye,
  incident: CircleAlert,
  deployment: ArrowUpRight,
  backup: Database,
  maintenance: Wrench,
  note: NotebookPen,
}
// Entries typed in well after the fact say so; the timeline keeps both moments.
const lateEntryMs = 5 * 60_000

const outcomeStyles: Record<RecordedEvent['outcome'], string> = {
  failure: 'border-rose-400/20 text-rose-300',
  warning: 'border-amber-400/20 text-amber-300',
  success: 'text-emerald-300',
  info: 'text-muted-foreground',
}

function EventIcon({ event }: { event: RecordedEvent }) {
  const Icon = kindIcons[event.kind]
  return (
    <span
      className={cn(
        'mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-lg border',
        outcomeStyles[event.outcome],
      )}
    >
      <Icon className="size-3.5" aria-hidden="true" />
      <span className="sr-only">{eventOutcomeLabels[event.outcome]}</span>
    </span>
  )
}

function EventItem({
  event,
  timeOnly,
  detailed,
}: {
  event: RecordedEvent
  timeOnly: boolean
  detailed: boolean
}) {
  const late = Date.parse(event.recordedAt) - Date.parse(event.occurredAt) > lateEntryMs
  return (
    <li className="flex gap-3 py-4 first:pt-1 last:pb-1">
      <EventIcon event={event} />
      <div className="min-w-0 flex-1">
        <div className="flex items-start justify-between gap-3">
          <p className="text-[13px] font-medium">{event.title}</p>
          <time
            dateTime={event.occurredAt}
            className="shrink-0 font-mono text-[10px] text-muted-foreground"
          >
            {timeOnly ? formatTime(event.occurredAt) : formatDateTime(event.occurredAt)}
          </time>
        </div>
        <p className="mt-1 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-xs text-muted-foreground">
          <SubjectLink subject={event.subject} />
          <span aria-hidden="true">·</span>
          <span>{eventKindLabels[event.kind]}</span>
          <SourceTag source={event.source} />
          {late && (
            <span className="text-[11px]">
              recorded <time dateTime={event.recordedAt}>{formatDateTime(event.recordedAt)}</time>
            </span>
          )}
        </p>
        {detailed && event.detail && (
          <p className="mt-2 text-xs leading-relaxed text-muted-foreground">{event.detail}</p>
        )}
      </div>
    </li>
  )
}

/** Consecutive automated results that said the same thing, folded into one row. */
function RepeatedResults({
  events,
  timeOnly,
  detailed,
  continues,
}: {
  events: RecordedEvent[]
  timeOnly: boolean
  detailed: boolean
  /** The run reaches where the view stops being complete, so it may go on before. */
  continues: boolean
}) {
  const [open, setOpen] = useState(false)
  const latest = events[0]!
  const oldest = events.at(-1)!
  const format = timeOnly ? formatTime : formatDateTime
  return (
    <li className="flex gap-3 py-4 first:pt-1 last:pb-1">
      <EventIcon event={latest} />
      <div className="min-w-0 flex-1">
        <div className="flex items-start justify-between gap-3">
          <p className="text-[13px] font-medium">{latest.title}</p>
          <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
            <time dateTime={oldest.occurredAt}>{format(oldest.occurredAt)}</time>–
            <time dateTime={latest.occurredAt}>{format(latest.occurredAt)}</time>
          </span>
        </div>
        <p className="mt-1 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-xs text-muted-foreground">
          <SubjectLink subject={latest.subject} />
          <span aria-hidden="true">·</span>
          <span>
            {events.length}
            {continues && '+'} consecutive results
          </span>
          <SourceTag source={latest.source} />
          <Button size="xs" variant="ghost" aria-expanded={open} onClick={() => setOpen(!open)}>
            {open ? 'Hide them' : `Show all ${events.length}`}
          </Button>
        </p>
        {detailed && latest.detail && !open && (
          <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
            Latest: {latest.detail}
          </p>
        )}
        {open && (
          <ol
            aria-label={`${events.length} consecutive results`}
            className="mt-2 divide-y divide-border/65 border-l pl-4"
          >
            {events.map((event) => (
              <EventItem key={event.id} event={event} timeOnly={timeOnly} detailed={detailed} />
            ))}
          </ol>
        )}
      </div>
    </li>
  )
}

export function EventRecordList({
  items,
  timeOnly = false,
  detailed = true,
  automatedSince = null,
  foldRepeats = false,
  maxRows,
  reachesCut = true,
}: {
  items: RecordedEvent[]
  timeOnly?: boolean
  detailed?: boolean
  /** Complete only from this moment on: older automated events are left out, and marked. */
  automatedSince?: string | null
  /** Folds runs of identical automated results of one system into a single row. */
  foldRepeats?: boolean
  /** Shows only the newest rows, counted after folding. */
  maxRows?: number
  /**
   * Nothing complete follows this list before the view stops being complete. False for one
   * list among several (a day), when an older one still has complete rows.
   */
  reachesCut?: boolean
}) {
  const shown = items.filter((event) =>
    withinWindow(event.source, event.occurredAt, automatedSince),
  )
  const all: TimelineRow[] = foldRepeats
    ? timelineRows(shown)
    : shown.map((event) => ({ kind: 'event', event }))
  const first = (row: TimelineRow) => (row.kind === 'event' ? row.event : row.events[0]!)
  const boundary = all.findIndex((row) => beforeWindow(first(row).occurredAt, automatedSince))
  // A run that ends where the view stops being complete may have older results not shown.
  const lastComplete = boundary !== -1 ? boundary - 1 : reachesCut ? all.length - 1 : -1
  const rows = all.slice(0, maxRows)
  return (
    <ol className="divide-y divide-border/65">
      {rows.map((row, index) => (
        // A run keeps its oldest result as new ones arrive, so an expanded run stays open.
        <Fragment key={row.kind === 'event' ? row.event.id : row.events.at(-1)!.id}>
          {automatedSince && index === boundary && index > 0 && (
            <WindowBoundary since={automatedSince} />
          )}
          {row.kind === 'event' ? (
            <EventItem event={row.event} timeOnly={timeOnly} detailed={detailed} />
          ) : (
            <RepeatedResults
              events={row.events}
              timeOnly={timeOnly}
              detailed={detailed}
              continues={automatedSince !== null && index === lastComplete}
            />
          )}
        </Fragment>
      ))}
    </ol>
  )
}

const evidenceBar: Record<ObservedEvidence['state'], string> = {
  unhealthy: 'bg-rose-300/75',
  degraded: 'bg-amber-300/75',
  stale: 'bg-slate-300/50',
  healthy: 'bg-emerald-300/60',
}

export function AttentionList({ items }: { items: AttentionItem[] }) {
  const { now } = useInventory()
  return (
    <ul className="divide-y">
      {items.map((item) =>
        item.kind === 'incident' ? (
          <li key={`incident:${item.incident.id}`} className="flex items-start gap-4 px-5 py-4">
            <span
              className={cn(
                'mt-1 h-7 w-0.5 shrink-0 rounded-full',
                severityBar[item.incident.severity],
              )}
            />
            <div className="min-w-0 flex-1">
              <Link
                to={`/incidents?incident=${item.incident.id}`}
                className="rounded-sm text-[13px] font-medium hover:underline"
              >
                {item.incident.title}
              </Link>
              <p className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1.5 text-[11px] text-muted-foreground">
                <SeverityBadge severity={item.incident.severity} />
                <span>Incident · {incidentStatusLabels[item.incident.status]}</span>
                <SubjectLink subject={item.subject} />
                <SourceTag source={item.incident.source} />
                <span>
                  opened{' '}
                  <time dateTime={item.incident.openedAt}>
                    {relativeTime(item.incident.openedAt, now).toLowerCase()}
                  </time>
                </span>
              </p>
            </div>
          </li>
        ) : (
          <li
            key={`evidence:${subjectKey(item.subject)}`}
            className="flex items-start gap-4 px-5 py-4"
          >
            <span
              className={cn(
                'mt-1 h-7 w-0.5 shrink-0 rounded-full',
                evidenceBar[item.evidence.state],
              )}
            />
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <SubjectLink
                  subject={item.subject}
                  className="text-[13px] font-medium text-foreground"
                />
                <EvidenceBadge state={item.evidence.state} compact />
              </div>
              <EvidenceDetail evidence={item.evidence} now={now} />
            </div>
          </li>
        ),
      )}
    </ul>
  )
}
