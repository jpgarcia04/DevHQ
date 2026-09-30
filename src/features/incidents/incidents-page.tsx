import { useState } from 'react'
import { PageHeader } from '@/components/page-header'
import { incidents } from '@/data/mock/scenario'
import { activeIncidents } from '@/domain/selectors'
import { incidentStatusLabels, severityLabels } from '@/domain/labels'
import { IncidentList } from './incident-list'

export function IncidentsPage() {
  const [status, setStatus] = useState('active')
  const [severity, setSeverity] = useState('all')
  const filtered = incidents.filter(
    (item) =>
      (status === 'all' ||
        (status === 'active' ? item.status !== 'resolved' : item.status === status)) &&
      (severity === 'all' || item.severity === severity),
  )
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Focus on what matters"
        title="Incidents"
        description="Issues that need attention, connected to the systems they affect."
        action={
          <span className="text-xs text-amber-200">
            {activeIncidents(incidents).length} active incidents
          </span>
        }
      />
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
        <IncidentList items={filtered} />
      </div>
      <p className="text-xs text-muted-foreground">
        Select an incident to see its context and affected system. Statuses are part of the sample
        scenario.
      </p>
    </div>
  )
}
