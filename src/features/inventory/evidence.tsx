import { Fragment, useId, useState } from 'react'
import { ChevronDown, CircleDashed, History, PenLine, Radar, type LucideIcon } from 'lucide-react'
import { Link } from 'react-router-dom'
import { healthLabels, sourceLabels } from '@/domain/labels'
import type { EntityRef, HttpEvidence, Observation, RecordSource } from '@/domain/models'
import {
  isCurrent,
  subjectEvidence,
  subjectKey,
  subjectObservations,
  type Evidence,
  type EvidenceState,
} from '@/domain/evidence'
import { Button } from '@/components/ui/button'
import { formatDateTime, relativeTime, timeUntil } from '@/shared/time'
import { cn } from '@/shared/utils'
import { automatedSince, beforeWindow } from './history'
import { inventoryError } from './inventory-api'
import { useInventory } from './inventory-context'

// Absence and expiry get their own icon and words, so neither can pass for a result.
const evidenceStyles: Record<EvidenceState, { label: string; color: string; icon?: LucideIcon }> = {
  unobserved: { label: 'Not observed', color: 'text-muted-foreground', icon: CircleDashed },
  stale: { label: 'Observation expired', color: 'text-slate-300', icon: History },
  healthy: { label: healthLabels.healthy, color: 'text-emerald-300' },
  degraded: { label: healthLabels.degraded, color: 'text-amber-300' },
  unhealthy: { label: healthLabels.unhealthy, color: 'text-rose-300' },
}

export function EvidenceBadge({
  state,
  compact = false,
}: {
  state: EvidenceState
  compact?: boolean
}) {
  const { label, color, icon: Icon } = evidenceStyles[state]
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-2 text-xs',
        color,
        !compact && 'rounded-md border border-current/15 bg-current/5 px-2.5 py-1',
        !compact && Icon && 'border-dashed',
      )}
    >
      {Icon ? (
        <Icon className="size-3" aria-hidden="true" />
      ) : (
        <span className="health-dot" aria-hidden="true" />
      )}
      {label}
    </span>
  )
}

// Who produced a record. Each source has its own words, icon and outline, and neither uses
// a health colour, so provenance can never be read as a result.
const sourceStyles: Record<RecordSource, { icon: LucideIcon; className: string }> = {
  manual: { icon: PenLine, className: 'border-dashed text-muted-foreground' },
  http: { icon: Radar, className: 'border-sky-300/30 bg-sky-300/5 text-sky-200' },
}

export function SourceTag({ source }: { source: RecordSource }) {
  const { icon: Icon, className } = sourceStyles[source]
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-1 rounded border px-1.5 py-0.5 text-[10px]',
        className,
      )}
    >
      <Icon className="size-2.5" aria-hidden="true" />
      {sourceLabels[source]}
    </span>
  )
}

const milliseconds = new Intl.NumberFormat('en-GB')

/** What an HTTP check requested and what came back. Bodies and headers are never kept. */
export function HttpEvidenceDetail({ evidence }: { evidence: HttpEvidence }) {
  const answered = evidence.status !== null && evidence.error === null
  return (
    <dl
      aria-label="HTTP check evidence"
      className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 rounded-md border border-sky-300/10 bg-sky-300/[0.03] px-3 py-2 text-[11px] text-muted-foreground"
    >
      <dt>Request</dt>
      <dd className="break-all font-mono text-foreground/85">GET {evidence.url}</dd>
      <dt>Status</dt>
      <dd>
        <span className="text-foreground/85">
          {evidence.status === null ? 'No response' : `HTTP ${evidence.status}`}
        </span>{' '}
        · expected {evidence.expectedStatus}
      </dd>
      <dt>Time</dt>
      <dd>
        {answered ? '' : 'failed after '}
        {milliseconds.format(evidence.latencyMs)} ms
      </dd>
      {evidence.error && (
        <>
          <dt>Error</dt>
          <dd className="break-words text-rose-200/90">{evidence.error}</dd>
        </>
      )}
      {evidence.redirects.length > 0 && (
        <>
          <dt>Redirects</dt>
          <dd>
            <ol className="space-y-0.5">
              {evidence.redirects.map((hop, index) => (
                <li key={index} className="break-all font-mono">
                  {hop.status} → {hop.location}
                </li>
              ))}
            </ol>
          </dd>
        </>
      )}
    </dl>
  )
}

function Moment({ value }: { value: string }) {
  return <time dateTime={value}>{formatDateTime(value)}</time>
}

/** Says what a view leaves out, so that a short history never reads as all there is. */
export function AutomatedWindowNote({ since, limit }: { since: string; limit: number }) {
  return (
    <p className="rounded-md border border-dashed px-3 py-2 text-[11px] leading-5 text-muted-foreground">
      Automated HTTP check results before <Moment value={since} /> are kept in the local database
      but not loaded: DevHQ loads the latest {limit} per system. Manual records are always complete.
    </p>
  )
}

/** Marks where a list stops being complete: older entries are manual records only. */
export function WindowBoundary({ since }: { since: string }) {
  return (
    <li className="py-3 text-center text-[10px] text-muted-foreground">
      Before <Moment value={since} /> · manual records only
    </li>
  )
}

/**
 * Provenance of an observation: who or what produced it, when it was seen, how long it
 * holds and, for an HTTP check, the technical evidence.
 */
function ObservationMeta({ observation, now }: { observation: Observation; now: number }) {
  return (
    <>
      <ObservationTimes observation={observation} now={now} />
      {observation.http && <HttpEvidenceDetail evidence={observation.http} />}
    </>
  )
}

function ObservationTimes({ observation, now }: { observation: Observation; now: number }) {
  return (
    <p className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
      <SourceTag source={observation.source} />
      <span>
        Observed <Moment value={observation.observedAt} /> (
        {relativeTime(observation.observedAt, now).toLowerCase()})
      </span>
      <span aria-hidden="true">·</span>
      {isCurrent(observation, now) ? (
        <span>
          Valid until <Moment value={observation.validUntil} /> (
          {timeUntil(observation.validUntil, now)})
        </span>
      ) : (
        <span>
          Expired <Moment value={observation.validUntil} /> (
          {relativeTime(observation.validUntil, now).toLowerCase()})
        </span>
      )}
    </p>
  )
}

export function EvidenceDetail({ evidence, now }: { evidence: Evidence; now: number }) {
  if (!evidence.latest)
    return (
      <p className="mt-2 text-xs leading-5 text-muted-foreground">
        No observation recorded. DevHQ does not treat this as healthy.
      </p>
    )
  return (
    <div>
      <p className="mt-2 text-xs leading-5 text-muted-foreground">
        {evidence.state === 'stale' && (
          <>
            Last result:{' '}
            <span className="text-foreground">{healthLabels[evidence.latest.result]}</span>.{' '}
          </>
        )}
        {evidence.latest.summary}
      </p>
      <ObservationMeta observation={evidence.latest} now={now} />
    </div>
  )
}

export function SubjectLink({ subject, className }: { subject: EntityRef; className?: string }) {
  const { snapshot } = useInventory()
  const name = (
    subject.type === 'application' ? snapshot?.applications : snapshot?.resources
  )?.find((item) => item.id === subject.id)?.name
  return (
    <Link
      className={cn(
        'rounded-sm text-muted-foreground hover:text-primary hover:underline',
        className,
      )}
      to={
        subject.type === 'application'
          ? `/applications/${subject.id}`
          : `/infrastructure?resource=${subject.id}`
      }
    >
      {name ?? `Unknown ${subject.type}`}
    </Link>
  )
}

/** Options for choosing an application or resource; values are subject keys. */
export function SubjectOptions() {
  const { snapshot } = useInventory()
  return (
    <>
      <optgroup label="Applications">
        {snapshot?.applications.map((app) => (
          <option value={subjectKey({ type: 'application', id: app.id })} key={app.id}>
            {app.name}
          </option>
        ))}
      </optgroup>
      <optgroup label="Infrastructure">
        {snapshot?.resources.map((resource) => (
          <option value={subjectKey({ type: 'resource', id: resource.id })} key={resource.id}>
            {resource.name}
          </option>
        ))}
      </optgroup>
    </>
  )
}

export function SubjectEvidence({ subject }: { subject: EntityRef }) {
  const { snapshot, now } = useInventory()
  const evidence = subjectEvidence(snapshot?.observations ?? [], subject, now)
  return (
    <div>
      <EvidenceBadge state={evidence.state} />
      <EvidenceDetail evidence={evidence} now={now} />
    </div>
  )
}

export function ObservationHistory({ subject }: { subject: EntityRef }) {
  // Older results loaded for one system never carry over to another.
  return <SubjectObservationHistory key={subjectKey(subject)} subject={subject} />
}

/**
 * A system's whole observation history behind a toggle, for places that show only its latest
 * evidence. Closed, it reads nothing; closing it forgets the older results it loaded.
 */
export function ObservationHistoryToggle({ subject, name }: { subject: EntityRef; name: string }) {
  const [open, setOpen] = useState(false)
  const region = useId()
  return (
    <div>
      <Button
        size="sm"
        variant="ghost"
        className="-ml-2"
        aria-expanded={open}
        aria-controls={region}
        onClick={() => setOpen(!open)}
      >
        <ChevronDown
          className={cn('size-3.5 transition-transform', open && 'rotate-180')}
          aria-hidden="true"
        />
        {open ? 'Hide observation history' : 'Show observation history'}
      </Button>
      <div
        id={region}
        role="region"
        aria-label={`Observation history of ${name}`}
        hidden={!open}
        className="mt-3"
      >
        {open && <ObservationHistory subject={subject} />}
      </div>
    </div>
  )
}

const byId = (items: Observation[]) => [...new Map(items.map((item) => [item.id, item])).values()]

function SubjectObservationHistory({ subject }: { subject: EntityRef }) {
  const { snapshot, now, olderObservations } = useInventory()
  const [older, setOlder] = useState<Observation[]>([])
  const [more, setMore] = useState(true)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // The subject's own observations first: the merge never touches other systems' records.
  const history = subjectObservations(
    byId([...subjectObservations(snapshot?.observations ?? [], subject), ...older]),
    subject,
  )
  if (!snapshot || !history.length)
    return (
      <p className="text-xs leading-5 text-muted-foreground">
        No observations recorded. Record what you have checked, or run this system’s HTTP check.
      </p>
    )
  const automated = history.filter((item) => item.source === 'http')
  const oldestAutomated = automated.at(-1)
  // Until older results are asked for, the snapshot says where its window starts; after,
  // the oldest loaded result does, until none is left.
  const windowSince = automatedSince(snapshot.automatedHistory, [subject])
  const since = !older.length ? windowSince : more ? (oldestAutomated?.observedAt ?? null) : null
  const boundary = history.findIndex((item) => beforeWindow(item.observedAt, since))
  const loadOlder = async () => {
    if (!oldestAutomated) return
    setLoading(true)
    setError(null)
    try {
      const page = await olderObservations(subject, oldestAutomated.id)
      // Keep what is shown now as well: a newer snapshot may drop it from its window.
      setOlder((current) => byId([...current, ...automated, ...page.observations]))
      setMore(page.more)
    } catch (cause) {
      setError(inventoryError(cause))
    } finally {
      setLoading(false)
    }
  }
  return (
    <div className="space-y-4">
      {since && <AutomatedWindowNote since={since} limit={snapshot.automatedHistory.limit} />}
      <ol className="divide-y">
        {history.map((observation, index) => {
          const standing =
            index > 0 ? 'Superseded' : isCurrent(observation, now) ? 'Current' : 'Expired'
          return (
            <Fragment key={observation.id}>
              {since && index === boundary && index > 0 && <WindowBoundary since={since} />}
              <li className="py-4 first:pt-0 last:pb-0">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <EvidenceBadge state={observation.result} compact />
                  <span
                    className={cn(
                      'rounded border px-1.5 py-0.5 text-[10px]',
                      standing === 'Current'
                        ? 'text-foreground'
                        : 'border-dashed text-muted-foreground',
                    )}
                  >
                    {standing}
                  </span>
                </div>
                <p className="mt-2 text-xs leading-5 text-muted-foreground">
                  {observation.summary}
                </p>
                <ObservationMeta observation={observation} now={now} />
              </li>
            </Fragment>
          )
        })}
      </ol>
      <OlderResults
        limit={snapshot.automatedHistory.limit}
        offered={since !== null}
        loading={loading}
        error={error}
        complete={older.length > 0 && !more}
        scope="this system"
        onLoad={() => void loadOlder()}
      />
    </div>
  )
}

/**
 * Offers the automated results a view left out, a page at a time, and says when none is left.
 * A failed read is reported and leaves what is shown as it was.
 */
export function OlderResults({
  limit,
  offered,
  loading,
  error,
  complete,
  scope,
  onLoad,
}: {
  limit: number
  offered: boolean
  loading: boolean
  error: string | null
  complete: boolean
  /** Whose results: "this system", "these systems". */
  scope: string
  onLoad: () => void
}) {
  return (
    <>
      {offered && (
        <Button size="sm" variant="outline" disabled={loading} onClick={onLoad}>
          {loading ? 'Loading older results…' : `Show ${limit} older automated results`}
        </Button>
      )}
      {complete && (
        <p className="text-[11px] text-muted-foreground">
          Every automated result of {scope} is shown.
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs text-rose-300">
          {error}
        </p>
      )}
    </>
  )
}
