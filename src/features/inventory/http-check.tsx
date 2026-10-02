import { useState, type FormEvent } from 'react'
import { Pause, Radar, Timer, TimerOff, type LucideIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { sameSubject, subjectEvidence, subjectKey } from '@/domain/evidence'
import { healthLabels } from '@/domain/labels'
import type { EntityRef } from '@/domain/models'
import { formatDateTime, formatTime, relativeTime } from '@/shared/time'
import { cn } from '@/shared/utils'
import {
  inventoryError,
  type HttpCheck,
  type MonitorCheckStatus,
  type MonitorState,
} from './inventory-api'
import { useInventory } from './inventory-context'
import { monitorStatusOf, nextRunText } from './supervision'
import { textLimits } from './limits'

const timeoutOptions = [1, 2, 5, 10]
const validityOptions = [15, 60, 4 * 60, 24 * 60, 7 * 24 * 60]
const intervalOptions = [5, 10, 15, 30, 60]

function duration(minutes: number) {
  const [count, unit] =
    minutes % (24 * 60) === 0
      ? [minutes / (24 * 60), 'day']
      : minutes % 60 === 0
        ? [minutes / 60, 'hour']
        : [minutes, 'min']
  return unit === 'min' ? `${count} min` : `${count} ${unit}${count === 1 ? '' : 's'}`
}

// Supervision says whether DevHQ looks on its own, never what it found, so it uses no
// health colour.
const supervisionStyles: Record<MonitorState, { icon: LucideIcon; className: string }> = {
  active: { icon: Timer, className: 'border-primary/30 bg-primary/5 text-primary' },
  paused: { icon: Pause, className: 'border-dashed text-muted-foreground' },
  off: { icon: TimerOff, className: 'border-dashed text-muted-foreground' },
}

export function SupervisionTag({ check }: { check?: HttpCheck }) {
  const state = check?.monitor ?? 'off'
  const { icon: Icon, className } = supervisionStyles[state]
  const every = check?.monitorIntervalMinutes ? ` · every ${check.monitorIntervalMinutes} min` : ''
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-1 rounded border px-1.5 py-0.5 text-[10px]',
        className,
      )}
    >
      <Icon className="size-2.5" aria-hidden="true" />
      {state === 'active' ? 'Supervised' : state === 'paused' ? 'Paused' : 'Not supervised'}
      {state !== 'off' && every}
    </span>
  )
}

export function DevhqRunError({ error }: { error: NonNullable<MonitorCheckStatus['lastError']> }) {
  return (
    <p className="text-[11px] leading-5 break-words text-rose-300">
      DevHQ could not run it at {formatTime(error.at)}: {error.message} Nothing was recorded; it
      tries again after the next interval, not before.
    </p>
  )
}

/** Supervision of a configured check: its state, rhythm, last and next run. */
function Supervision({ check, name }: { check: HttpCheck; name: string }) {
  const { monitor, monitorError, now } = useInventory()
  const status = monitorStatusOf(monitor, check.subject)
  if (check.monitor === 'off')
    return (
      <p className="mt-1 text-[11px] text-muted-foreground">
        Not supervised: it runs only when you ask. Choose an interval in Edit check to have DevHQ
        run it while it is open.
      </p>
    )
  const next = check.monitor === 'active' ? nextRunText(status, now) : null
  return (
    <div
      role="group"
      aria-label={`Supervision of ${name}`}
      className="mt-2 space-y-1 rounded-md border px-3 py-2 text-[11px] text-muted-foreground"
    >
      <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <SupervisionTag check={check} />
        {check.monitor === 'paused' && <span>No run starts on its own until you resume it.</span>}
        {next && <span className="text-foreground/85">{next}</span>}
      </p>
      <p>
        Last run:{' '}
        {status?.lastRunAt ? (
          <>
            <time dateTime={status.lastRunAt}>{formatDateTime(status.lastRunAt)}</time> (
            {relativeTime(status.lastRunAt, now).toLowerCase()})
          </>
        ) : (
          'not yet'
        )}
      </p>
      {status?.lastError && <DevhqRunError error={status.lastError} />}
      {(monitorError || monitor?.error) && (
        <p className="break-words text-rose-300">
          Supervision status is unavailable: {monitorError ?? monitor?.error}
        </p>
      )}
      <p>Runs only while DevHQ is open. Missed runs are not made up; an overdue check runs once.</p>
    </div>
  )
}

/**
 * The subject's HTTP check: what it requests, whether DevHQ runs it on its own, and a way to
 * run it now. Its value is the observation each run records, shown with the rest of the
 * evidence, not the button.
 */
export function HttpCheckPanel({ subject, name }: { subject: EntityRef; name: string }) {
  const { snapshot, now, checking, monitor, runHttpCheck, removeHttpCheck, setHttpCheckPaused } =
    useInventory()
  const check = snapshot?.httpChecks.find((item) => sameSubject(item.subject, subject))
  const latest = subjectEvidence(snapshot?.observations ?? [], subject, now).latest
  const running =
    checking.includes(subjectKey(subject)) || !!monitorStatusOf(monitor, subject)?.running
  const [editing, setEditing] = useState(false)
  const [busy, setBusy] = useState(false)
  const [outcome, setOutcome] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const act = async (action: () => Promise<string | null>) => {
    setError(null)
    setOutcome(null)
    setBusy(true)
    try {
      setOutcome(await action())
    } catch (cause) {
      setError(inventoryError(cause))
    } finally {
      setBusy(false)
    }
  }
  const run = () =>
    act(async () => {
      const observation = await runHttpCheck(subject)
      return `Recorded as ${healthLabels[observation.result]}: ${observation.summary}.`
    })
  const remove = () =>
    act(async () => {
      await removeHttpCheck(subject)
      return 'HTTP check removed, with its supervision. Any run in progress was cancelled and recorded nothing. The observations it recorded before are kept.'
    })
  const pause = (paused: boolean) =>
    act(async () => {
      await setHttpCheckPaused(subject, paused)
      if (!paused) return 'Supervision resumed. If a run is overdue, it runs once now.'
      return running
        ? 'Supervision paused. The run in progress will finish and be recorded; no other run starts on its own.'
        : 'Supervision paused. No run starts on its own until you resume it.'
    })
  return (
    <section aria-label={`HTTP check for ${name}`} className="space-y-3">
      <div className="min-w-0">
        <h3 className="eyebrow">HTTP check</h3>
        {check ? (
          <>
            <p className="mt-2 font-mono text-[11px] break-words text-foreground/85">
              GET {check.url}
            </p>
            <p className="mt-1 text-[11px] text-muted-foreground">
              Expects {check.expectedStatus} · gives up after {check.timeoutSeconds} s · result
              valid for {duration(check.validForMinutes)}
            </p>
            {latest?.http && latest.http.url !== check.url && (
              <p className="mt-1 text-[11px] break-words text-amber-200/90">
                The current evidence checked {latest.http.url}; the URL has changed since.
              </p>
            )}
            <Supervision check={check} name={name} />
          </>
        ) : (
          <p className="mt-2 text-xs leading-5 text-muted-foreground">
            Not configured. An HTTP check requests one URL, when you ask or on a schedule while
            DevHQ is open, and records the answer as automated evidence with its own validity.
          </p>
        )}
      </div>
      <div className="flex flex-wrap gap-2">
        {check && (
          <Button size="sm" variant="outline" disabled={running} onClick={() => void run()}>
            <Radar className="size-3.5" aria-hidden="true" />
            {running ? 'Checking…' : 'Run check'}
          </Button>
        )}
        {check && check.monitor !== 'off' && (
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => void pause(check.monitor === 'active')}
          >
            {check.monitor === 'active' ? 'Pause supervision' : 'Resume supervision'}
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          aria-expanded={editing}
          onClick={() => setEditing(!editing)}
        >
          {check ? 'Edit check' : 'Configure HTTP check'}
        </Button>
        {check && (
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => void remove()}>
            Remove check
          </Button>
        )}
      </div>
      <p role="status" className="text-[11px] text-muted-foreground empty:hidden">
        {running && !outcome
          ? `Checking for up to ${check?.timeoutSeconds ?? 10} s. You can keep working.`
          : outcome}
      </p>
      {error && (
        <p role="alert" className="text-xs text-rose-300">
          {error}
        </p>
      )}
      {editing && (
        <HttpCheckForm
          subject={subject}
          check={check}
          onDone={() => setEditing(false)}
          // What the last run or action said was about the check as it was: no longer the one shown.
          onSaved={() => {
            setOutcome(null)
            setError(null)
          }}
        />
      )}
    </section>
  )
}

export function HttpCheckForm({
  subject,
  check,
  onDone,
  onSaved,
}: {
  subject: EntityRef
  check?: HttpCheck
  onDone: () => void
  /** Called once the new configuration is saved, before `onDone`. */
  onSaved?: () => void
}) {
  const { saveHttpCheck } = useInventory()
  const [url, setUrl] = useState(check?.url ?? '')
  const [expectedStatus, setExpectedStatus] = useState(String(check?.expectedStatus ?? 200))
  const [timeoutSeconds, setTimeoutSeconds] = useState(check?.timeoutSeconds ?? 5)
  const [validForMinutes, setValidForMinutes] = useState(check?.validForMinutes ?? 60)
  const [interval, setMonitorInterval] = useState<number | null>(
    check?.monitorIntervalMinutes ?? null,
  )
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const lapses = interval !== null && validForMinutes <= interval
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await saveHttpCheck({
        subject,
        url,
        expectedStatus: Number(expectedStatus),
        timeoutSeconds,
        validForMinutes,
        monitorIntervalMinutes: interval,
      })
      onSaved?.()
      onDone()
    } catch (cause) {
      setError(inventoryError(cause))
    } finally {
      setBusy(false)
    }
  }
  return (
    <form
      className="space-y-4 rounded-lg border p-4"
      aria-label="Configure HTTP check"
      onSubmit={(event) => void submit(event)}
    >
      <p className="text-xs leading-5 text-muted-foreground">
        A read-only GET sent by DevHQ itself, not by this window. No credentials, headers or body
        are sent, and certificates are always verified. Up to 5 redirects are followed and recorded.
        The URL is stored in plain text: keep secrets out of it.
      </p>
      <div className="grid gap-4 min-[1100px]:grid-cols-2">
        <label className="grid gap-1.5 text-xs min-[1100px]:col-span-2">
          URL
          <Input
            required
            spellCheck={false}
            type="url"
            maxLength={textLimits.url}
            placeholder="https://example.com/health"
            value={url}
            onChange={(event) => setUrl(event.target.value)}
          />
        </label>
        <label className="grid gap-1.5 text-xs">
          Expected status
          <Input
            required
            type="number"
            min={200}
            max={599}
            value={expectedStatus}
            onChange={(event) => setExpectedStatus(event.target.value)}
          />
        </label>
        <label className="grid gap-1.5 text-xs">
          Give up after
          <select
            className="filter-select block w-full"
            value={timeoutSeconds}
            onChange={(event) => setTimeoutSeconds(Number(event.target.value))}
          >
            {timeoutOptions.map((seconds) => (
              <option value={seconds} key={seconds}>
                {seconds} s
              </option>
            ))}
          </select>
        </label>
        <label className="grid gap-1.5 text-xs">
          Result valid for
          <select
            className="filter-select block w-full"
            value={validForMinutes}
            onChange={(event) => setValidForMinutes(Number(event.target.value))}
          >
            {validityOptions.map((minutes) => (
              <option value={minutes} key={minutes}>
                {duration(minutes)}
              </option>
            ))}
          </select>
        </label>
        <label className="grid gap-1.5 text-xs">
          Supervise while DevHQ is open
          <select
            className="filter-select block w-full"
            value={interval ?? ''}
            aria-describedby="supervision-hint"
            onChange={(event) =>
              setMonitorInterval(event.target.value ? Number(event.target.value) : null)
            }
          >
            <option value="">Off · only when I ask</option>
            {intervalOptions.map((minutes) => (
              <option value={minutes} key={minutes}>
                Every {minutes} min
              </option>
            ))}
          </select>
        </label>
      </div>
      <p
        id="supervision-hint"
        className={cn('text-[11px] leading-5', lapses ? 'text-amber-200' : 'text-muted-foreground')}
      >
        {lapses
          ? `The result must stay valid for longer than the interval (${interval} min), or the evidence would expire before the next run.`
          : 'Supervision runs only while DevHQ is open, never in the background, and never retries. Each result must stay valid for longer than the interval.'}
      </p>
      {error && (
        <p role="alert" className="text-xs text-rose-300">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="submit" disabled={busy}>
          {busy ? 'Saving…' : 'Save HTTP check'}
        </Button>
        <Button type="button" variant="outline" onClick={onDone}>
          Cancel
        </Button>
      </div>
    </form>
  )
}
