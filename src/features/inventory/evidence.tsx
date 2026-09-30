import { CircleDashed, History, PenLine, type LucideIcon } from 'lucide-react'
import { Link } from 'react-router-dom'
import { healthLabels, sourceLabels } from '@/domain/labels'
import type { EntityRef, Observation, RecordSource } from '@/domain/models'
import {
  isCurrent,
  subjectEvidence,
  subjectKey,
  subjectObservations,
  type Evidence,
  type EvidenceState,
} from '@/domain/evidence'
import { formatDateTime, relativeTime, timeUntil } from '@/shared/time'
import { cn } from '@/shared/utils'
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

export function SourceTag({ source }: { source: RecordSource }) {
  return (
    <span className="inline-flex items-center gap-1 rounded border border-dashed px-1.5 py-0.5 text-[10px] text-muted-foreground">
      <PenLine className="size-2.5" aria-hidden="true" />
      {sourceLabels[source]}
    </span>
  )
}

function Moment({ value }: { value: string }) {
  return <time dateTime={value}>{formatDateTime(value)}</time>
}

/** Provenance of an observation: who reported it, when it was seen and how long it holds. */
function ObservationMeta({ observation, now }: { observation: Observation; now: number }) {
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
  const { snapshot, now } = useInventory()
  const history = subjectObservations(snapshot?.observations ?? [], subject)
  if (!history.length)
    return (
      <p className="text-xs leading-5 text-muted-foreground">
        No observations recorded. Use Record observation to add what you have checked.
      </p>
    )
  return (
    <ol className="divide-y">
      {history.map((observation, index) => {
        const standing =
          index > 0 ? 'Superseded' : isCurrent(observation, now) ? 'Current' : 'Expired'
        return (
          <li key={observation.id} className="py-4 first:pt-0 last:pb-0">
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
            <p className="mt-2 text-xs leading-5 text-muted-foreground">{observation.summary}</p>
            <ObservationMeta observation={observation} now={now} />
          </li>
        )
      })}
    </ol>
  )
}
