import { useState } from 'react'
import { Plus } from 'lucide-react'
import { useSearchParams } from 'react-router-dom'
import { EmptyState } from '@/components/empty-state'
import { PageHeader } from '@/components/page-header'
import { Button } from '@/components/ui/button'
import { subjectFromKey, subjectsOf } from '@/domain/evidence'
import { incidentStatusLabels, severityLabels } from '@/domain/labels'
import { activeIncidents } from '@/domain/selectors'
import { deviceTimeZone, formatDate } from '@/shared/time'
import { AutomatedWindowNote, OlderResults, SubjectOptions } from './evidence'
import { beforeWindow, withinWindow } from './history'
import { useInventory } from './inventory-context'
import { InventoryState } from './inventory-state'
import { EventForm, IncidentForm } from './record-forms'
import { EventRecordList, IncidentRecordList } from './record-lists'
import { useTimeline } from './timeline'

export function RealIncidentsPage() {
  const { snapshot, loadState } = useInventory()
  const [params] = useSearchParams()
  const linked = params.get('incident')
  // A linked incident may already be resolved; show every status so it is not hidden.
  const [status, setStatus] = useState(linked ? 'all' : 'active')
  const [severity, setSeverity] = useState('all')
  const [creating, setCreating] = useState(false)
  const incidents = snapshot?.incidents ?? []
  const active = activeIncidents(incidents).length
  const filtered = incidents.filter(
    (item) =>
      (status === 'all' ||
        (status === 'active' ? item.status !== 'resolved' : item.status === status)) &&
      (severity === 'all' || item.severity === severity),
  )
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory · manual records"
        title="Incidents"
        description="Problems you have recorded, connected to the systems they affect."
        action={
          <div className="flex items-center gap-4">
            {snapshot && loadState !== 'error' && (
              <span className="text-xs text-amber-200">
                {active} active {active === 1 ? 'incident' : 'incidents'}
              </span>
            )}
            <Button onClick={() => setCreating(true)}>
              <Plus className="size-4" /> Open incident
            </Button>
          </div>
        }
      />
      <InventoryState>
        {creating && <IncidentForm onDone={() => setCreating(false)} />}
        {incidents.length ? (
          <>
            <div className="flex flex-wrap items-center gap-3">
              <select
                aria-label="Incident status"
                value={status}
                onChange={(event) => setStatus(event.target.value)}
                className="filter-select"
              >
                <option value="active">Active incidents</option>
                <option value="all">All statuses</option>
                {Object.entries(incidentStatusLabels).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
              <select
                aria-label="Incident severity"
                value={severity}
                onChange={(event) => setSeverity(event.target.value)}
                className="filter-select"
              >
                <option value="all">All severities</option>
                {Object.entries(severityLabels).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
              <span className="ml-auto text-xs text-muted-foreground" aria-live="polite">
                {filtered.length} shown
              </span>
            </div>
            <div className="panel overflow-hidden">
              {filtered.length ? (
                <IncidentRecordList items={filtered} expanded={linked} />
              ) : (
                <p className="py-8 text-center text-sm text-muted-foreground">
                  No incidents in this view.
                </p>
              )}
            </div>
            <p className="text-xs text-muted-foreground">
              Every incident here was opened by hand in DevHQ. No monitor creates incidents yet.
            </p>
          </>
        ) : (
          <EmptyState
            title="No incidents recorded"
            description="Open an incident when you identify a problem. An empty list is not evidence that everything is healthy."
          />
        )}
      </InventoryState>
    </div>
  )
}

export function RealActivityPage() {
  const { snapshot, now } = useInventory()
  const [subject, setSubject] = useState('all')
  const [creating, setCreating] = useState(false)
  const chosen = subjectFromKey(subject)
  const timeline = useTimeline(subject, chosen ? [chosen] : snapshot ? subjectsOf(snapshot) : [])
  const since = timeline.since
  const events = timeline.events.filter((event) =>
    withinWindow(event.source, event.occurredAt, since),
  )
  const days = [...new Set(events.map((event) => formatDate(event.occurredAt)))]
  const today = formatDate(new Date(now).toISOString())
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory · recorded activity"
        title="Activity"
        description="Observations, incident changes and events recorded in DevHQ, newest first."
        action={
          <Button onClick={() => setCreating(true)}>
            <Plus className="size-4" /> Log event
          </Button>
        }
      />
      <InventoryState>
        {creating && <EventForm onDone={() => setCreating(false)} />}
        {snapshot?.events.length ? (
          <>
            <div className="flex flex-wrap items-center justify-between gap-3">
              <select
                aria-label="Filter activity by system"
                className="filter-select"
                value={subject}
                onChange={(event) => setSubject(event.target.value)}
              >
                <option value="all">All systems</option>
                <SubjectOptions />
              </select>
              <span className="text-[11px] text-muted-foreground">
                {deviceTimeZone()} · device time
              </span>
            </div>
            <div className="max-w-4xl space-y-7">
              {since && snapshot && (
                <AutomatedWindowNote since={since} limit={snapshot.automatedHistory.limit} />
              )}
              {days.map((day) => {
                const items = events.filter((event) => formatDate(event.occurredAt) === day)
                // A day entirely before the window holds manual records only; say so.
                const manualOnly = items.every((event) => beforeWindow(event.occurredAt, since))
                return (
                  <section key={day} aria-label={day}>
                    <h2 className="eyebrow mb-4">
                      {day === today ? `Today · ${day}` : day}
                      {manualOnly && ' · manual records only'}
                    </h2>
                    <div className="panel px-5 py-5">
                      <EventRecordList timeOnly foldRepeats items={items} automatedSince={since} />
                    </div>
                  </section>
                )
              })}
              {!events.length && (
                <p className="py-10 text-muted-foreground">No activity recorded for this system.</p>
              )}
              <div className="space-y-3 empty:hidden">
                <OlderResults
                  limit={timeline.limit}
                  offered={since !== null}
                  loading={timeline.loading}
                  error={timeline.error}
                  complete={timeline.complete}
                  scope={chosen ? 'this system' : 'these systems'}
                  onLoad={() => void timeline.loadOlder()}
                />
              </div>
            </div>
          </>
        ) : (
          <EmptyState
            title="No activity recorded"
            description="Record an observation, run an HTTP check, open an incident or log an event. DevHQ does not collect activity in the background."
          />
        )}
      </InventoryState>
    </div>
  )
}
