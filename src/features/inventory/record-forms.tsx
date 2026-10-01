import { useState, type FormEvent, type ReactNode } from 'react'
import { CircleAlert, Eye, NotebookPen } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { subjectFromKey, subjectKey } from '@/domain/evidence'
import { eventKindLabels, eventOutcomeLabels, healthLabels, severityLabels } from '@/domain/labels'
import type {
  EntityRef,
  EventOutcome,
  IncidentSeverity,
  ManualEventKind,
  ObservationResult,
} from '@/domain/models'
import { toDateTimeLocal } from '@/shared/time'
import { SubjectOptions } from './evidence'
import { inventoryError } from './inventory-api'
import { useInventory } from './inventory-context'
import { textLimits } from './limits'

const validityOptions = [
  { minutes: 60, label: '1 hour' },
  { minutes: 4 * 60, label: '4 hours' },
  { minutes: 24 * 60, label: '24 hours' },
  { minutes: 7 * 24 * 60, label: '7 days' },
  { minutes: 30 * 24 * 60, label: '30 days' },
]
const observationResults: ObservationResult[] = ['healthy', 'degraded', 'unhealthy']
const manualEventKinds: ManualEventKind[] = ['deployment', 'backup', 'maintenance', 'note']

function useSubmission(onDone: () => void) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const submit = (write: () => Promise<void>) => async (event: FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await write()
      onDone()
    } catch (cause) {
      setError(inventoryError(cause))
    } finally {
      setBusy(false)
    }
  }
  return { busy, error, submit }
}

function requireSubject(key: string) {
  const subject = subjectFromKey(key)
  if (!subject) throw new Error('Choose the application or resource this record is about.')
  return subject
}

function requireTime(value: string, label: string) {
  const time = new Date(value)
  if (Number.isNaN(time.getTime())) throw new Error(`Enter a valid ${label}.`)
  return time.toISOString()
}

function RecordForm({
  title,
  notice,
  submitLabel,
  subject,
  onSubjectChange,
  busy,
  error,
  onSubmit,
  onCancel,
  children,
}: {
  title: string
  notice: string
  submitLabel: string
  subject: string
  onSubjectChange: ((key: string) => void) | null
  busy: boolean
  error: string | null
  onSubmit: (event: FormEvent) => void
  onCancel: () => void
  children: ReactNode
}) {
  const { snapshot } = useInventory()
  const empty = !snapshot?.applications.length && !snapshot?.resources.length
  return (
    <form className="panel space-y-4 p-5" aria-label={title} onSubmit={onSubmit}>
      <div>
        <h2 className="section-heading">{title}</h2>
        <p className="mt-1.5 text-xs leading-5 text-muted-foreground">{notice}</p>
      </div>
      <div className="grid gap-4 min-[1100px]:grid-cols-2">
        {onSubjectChange && (
          <label className="space-y-1.5 text-xs">
            System
            <select
              required
              className="filter-select block w-full"
              value={subject}
              onChange={(event) => onSubjectChange(event.target.value)}
            >
              <option value="" disabled>
                {empty ? 'Register an application or resource first' : 'Choose a system'}
              </option>
              <SubjectOptions />
            </select>
          </label>
        )}
        {children}
      </div>
      {error && (
        <p role="alert" className="text-xs text-rose-300">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="submit" disabled={busy}>
          {busy ? 'Saving…' : submitLabel}
        </Button>
        <Button type="button" variant="outline" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </form>
  )
}

export function ObservationForm({ subject, onDone }: { subject?: EntityRef; onDone: () => void }) {
  const { now, recordObservation } = useInventory()
  const [target, setTarget] = useState(subject ? subjectKey(subject) : '')
  const [result, setResult] = useState<ObservationResult>('healthy')
  const [summary, setSummary] = useState('')
  const [observedAt, setObservedAt] = useState(() => toDateTimeLocal(now))
  const [validFor, setValidFor] = useState(24 * 60)
  const { busy, error, submit } = useSubmission(onDone)
  return (
    <RecordForm
      title="Record observation"
      notice="Manual entry: what you checked yourself. DevHQ stores it as reported and does not run a check. When its validity ends it is shown as expired, not healthy."
      submitLabel="Save observation"
      subject={target}
      onSubjectChange={subject ? null : setTarget}
      busy={busy}
      error={error}
      onSubmit={submit(() =>
        recordObservation({
          subject: requireSubject(target),
          result,
          summary,
          observedAt: requireTime(observedAt, 'observation time'),
          validForMinutes: validFor,
        }),
      )}
      onCancel={onDone}
    >
      <label className="space-y-1.5 text-xs">
        Result
        <select
          className="filter-select block w-full"
          value={result}
          onChange={(event) => setResult(event.target.value as ObservationResult)}
        >
          {observationResults.map((value) => (
            <option value={value} key={value}>
              {healthLabels[value]}
            </option>
          ))}
        </select>
      </label>
      <label className="space-y-1.5 text-xs min-[1100px]:col-span-2">
        What did you observe?
        <Input
          required
          maxLength={textLimits.summary}
          placeholder="e.g. Login page responds; disk at 71%"
          value={summary}
          onChange={(event) => setSummary(event.target.value)}
        />
      </label>
      <label className="space-y-1.5 text-xs">
        Observed at
        <Input
          required
          type="datetime-local"
          max={toDateTimeLocal(now)}
          value={observedAt}
          onChange={(event) => setObservedAt(event.target.value)}
        />
      </label>
      <label className="space-y-1.5 text-xs">
        Valid for
        <select
          className="filter-select block w-full"
          value={validFor}
          onChange={(event) => setValidFor(Number(event.target.value))}
        >
          {validityOptions.map(({ minutes, label }) => (
            <option value={minutes} key={minutes}>
              {label}
            </option>
          ))}
        </select>
      </label>
    </RecordForm>
  )
}

export function IncidentForm({ subject, onDone }: { subject?: EntityRef; onDone: () => void }) {
  const { openIncident } = useInventory()
  const [target, setTarget] = useState(subject ? subjectKey(subject) : '')
  const [title, setTitle] = useState('')
  const [severity, setSeverity] = useState<IncidentSeverity>('warning')
  const [description, setDescription] = useState('')
  const { busy, error, submit } = useSubmission(onDone)
  return (
    <RecordForm
      title="Open incident"
      notice="Manual entry: a problem you have identified. It stays open until you resolve it."
      submitLabel="Open incident"
      subject={target}
      onSubjectChange={subject ? null : setTarget}
      busy={busy}
      error={error}
      onSubmit={submit(() =>
        openIncident({ subject: requireSubject(target), title, severity, description }),
      )}
      onCancel={onDone}
    >
      <label className="space-y-1.5 text-xs">
        Title
        <Input
          required
          maxLength={textLimits.title}
          value={title}
          onChange={(event) => setTitle(event.target.value)}
        />
      </label>
      <label className="space-y-1.5 text-xs">
        Severity
        <select
          className="filter-select block w-full"
          value={severity}
          onChange={(event) => setSeverity(event.target.value as IncidentSeverity)}
        >
          {Object.entries(severityLabels).map(([value, label]) => (
            <option value={value} key={value}>
              {label}
            </option>
          ))}
        </select>
      </label>
      <label className="space-y-1.5 text-xs min-[1100px]:col-span-2">
        Description
        <Input
          maxLength={textLimits.description}
          value={description}
          onChange={(event) => setDescription(event.target.value)}
        />
      </label>
    </RecordForm>
  )
}

export function EventForm({ subject, onDone }: { subject?: EntityRef; onDone: () => void }) {
  const { now, recordEvent } = useInventory()
  const [target, setTarget] = useState(subject ? subjectKey(subject) : '')
  const [kind, setKind] = useState<ManualEventKind>('deployment')
  const [outcome, setOutcome] = useState<EventOutcome>('success')
  const [title, setTitle] = useState('')
  const [detail, setDetail] = useState('')
  const [occurredAt, setOccurredAt] = useState(() => toDateTimeLocal(now))
  const { busy, error, submit } = useSubmission(onDone)
  return (
    <RecordForm
      title="Log event"
      notice="Manual entry: something that happened, such as a deployment you ran. It becomes part of the activity timeline."
      submitLabel="Save event"
      subject={target}
      onSubjectChange={subject ? null : setTarget}
      busy={busy}
      error={error}
      onSubmit={submit(() =>
        recordEvent({
          subject: requireSubject(target),
          kind,
          outcome,
          title,
          detail,
          occurredAt: requireTime(occurredAt, 'event time'),
        }),
      )}
      onCancel={onDone}
    >
      <label className="space-y-1.5 text-xs">
        Kind
        <select
          className="filter-select block w-full"
          value={kind}
          onChange={(event) => setKind(event.target.value as ManualEventKind)}
        >
          {manualEventKinds.map((value) => (
            <option value={value} key={value}>
              {eventKindLabels[value]}
            </option>
          ))}
        </select>
      </label>
      <label className="space-y-1.5 text-xs">
        Outcome
        <select
          className="filter-select block w-full"
          value={outcome}
          onChange={(event) => setOutcome(event.target.value as EventOutcome)}
        >
          {Object.entries(eventOutcomeLabels).map(([value, label]) => (
            <option value={value} key={value}>
              {label}
            </option>
          ))}
        </select>
      </label>
      <label className="space-y-1.5 text-xs">
        Title
        <Input
          required
          maxLength={textLimits.title}
          value={title}
          onChange={(event) => setTitle(event.target.value)}
        />
      </label>
      <label className="space-y-1.5 text-xs">
        Happened at
        <Input
          required
          type="datetime-local"
          max={toDateTimeLocal(now)}
          value={occurredAt}
          onChange={(event) => setOccurredAt(event.target.value)}
        />
      </label>
      <label className="space-y-1.5 text-xs min-[1100px]:col-span-2">
        Details
        <Input
          maxLength={textLimits.description}
          value={detail}
          onChange={(event) => setDetail(event.target.value)}
        />
      </label>
    </RecordForm>
  )
}

type RecordKind = 'observation' | 'incident' | 'event'
const recordActions: { kind: RecordKind; label: string; icon: typeof Eye }[] = [
  { kind: 'observation', label: 'Record observation', icon: Eye },
  { kind: 'incident', label: 'Open incident', icon: CircleAlert },
  { kind: 'event', label: 'Log event', icon: NotebookPen },
]

/** Manual record entry for one application or resource. */
export function RecordActions({ subject, name }: { subject: EntityRef; name: string }) {
  const [open, setOpen] = useState<RecordKind | null>(null)
  const close = () => setOpen(null)
  return (
    <div className="space-y-4">
      <div
        role="group"
        aria-label={`Manual records for ${name}`}
        className="flex flex-wrap items-center gap-2"
      >
        {recordActions.map(({ kind, label, icon: Icon }) => (
          <Button
            key={kind}
            size="sm"
            variant={open === kind ? 'secondary' : 'outline'}
            aria-expanded={open === kind}
            onClick={() => setOpen(open === kind ? null : kind)}
          >
            <Icon className="size-3.5" aria-hidden="true" /> {label}
          </Button>
        ))}
      </div>
      {open === 'observation' && <ObservationForm subject={subject} onDone={close} />}
      {open === 'incident' && <IncidentForm subject={subject} onDone={close} />}
      {open === 'event' && <EventForm subject={subject} onDone={close} />}
    </div>
  )
}
