import { useState } from 'react'
import { PageHeader } from '@/components/page-header'
import { applications, events, resources, scenario } from '@/data/mock/scenario'
import { formatDate } from '@/shared/time'
import { ActivityTimeline } from './activity-timeline'

export function ActivityPage() {
  const [subject, setSubject] = useState('all')
  const filtered = events.filter(
    (event) => subject === 'all' || `${event.subject.type}:${event.subject.id}` === subject,
  )
  const days = [
    ...new Set(filtered.map((event) => formatDate(event.occurredAt, scenario.timeZone))),
  ]
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="The memory of your ecosystem"
        title="Activity"
        description="A shared timeline of deployments, builds, checks, backups and operational changes."
      />
      <div className="flex items-center justify-between">
        <select
          aria-label="Filter activity by system"
          className="filter-select"
          value={subject}
          onChange={(event) => setSubject(event.target.value)}
        >
          <option value="all">All systems</option>
          <optgroup label="Applications">
            {applications.map((app) => (
              <option value={`application:${app.id}`} key={app.id}>
                {app.name}
              </option>
            ))}
          </optgroup>
          <optgroup label="Infrastructure">
            {resources.map((resource) => (
              <option value={`resource:${resource.id}`} key={resource.id}>
                {resource.name}
              </option>
            ))}
          </optgroup>
        </select>
        <span className="text-[11px] text-muted-foreground">Europe/Madrid · CEST</span>
      </div>
      <div className="max-w-4xl space-y-7">
        {days.map((day) => (
          <section key={day}>
            <h2 className="eyebrow mb-4">
              {day === formatDate(scenario.observedAt, scenario.timeZone) ? `Today · ${day}` : day}
            </h2>
            <div className="panel px-5 py-5">
              <ActivityTimeline
                detailed
                items={filtered.filter(
                  (event) => formatDate(event.occurredAt, scenario.timeZone) === day,
                )}
              />
            </div>
          </section>
        ))}
        {!filtered.length && (
          <p className="py-10 text-muted-foreground">No activity recorded for this system.</p>
        )}
      </div>
    </div>
  )
}
