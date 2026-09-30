import { Fragment } from 'react'
import { sameSubject, subjectKey, subjectsOf } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import { SubjectLink } from './evidence'
import { DevhqRunError, SupervisionTag } from './http-check'
import type { HttpCheck } from './inventory-api'
import { useInventory } from './inventory-context'
import { monitorStatusOf, nextRunText } from './supervision'

/**
 * What DevHQ is checking on its own, and what it is not. Being supervised says that DevHQ
 * looks, never what it found: the evidence each run records decides that.
 */
export function SupervisionOverview() {
  const { snapshot, monitor, monitorError, now } = useInventory()
  if (!snapshot) return null
  const subjects = subjectsOf(snapshot)
  if (!subjects.length) return null
  const rows = subjects.map((subject) => ({
    subject,
    check: snapshot.httpChecks.find((item) => sameSubject(item.subject, subject)),
  }))
  const watched = rows.filter(
    (row): row is { subject: EntityRef; check: HttpCheck } =>
      !!row.check && row.check.monitor !== 'off',
  )
  const active = watched.filter((row) => row.check.monitor === 'active').length
  const unwatched = rows.filter((row) => row.check?.monitor !== 'active')
  const unavailable = monitorError ?? monitor?.error
  return (
    <section aria-labelledby="supervision-heading">
      <h2 id="supervision-heading" className="section-heading mb-3">
        Supervision
      </h2>
      <div className="panel space-y-4 p-5 text-xs">
        <div>
          <p className="font-medium">
            {active
              ? `${active} of ${subjects.length} ${subjects.length === 1 ? 'system is' : 'systems are'} supervised`
              : 'Nothing is supervised right now'}
          </p>
          <p className="mt-1.5 leading-5 text-muted-foreground">
            {active
              ? 'DevHQ runs their HTTP checks on a schedule while it is open. Being supervised is not being healthy: what each run records decides that.'
              : 'DevHQ is not checking anything on its own. That says nothing about health: only current evidence does.'}
          </p>
        </div>
        {watched.length > 0 && (
          <ul className="divide-y border-y">
            {watched.map(({ subject, check }) => {
              const status = monitorStatusOf(monitor, subject)
              const next = check.monitor === 'active' ? nextRunText(status, now) : null
              return (
                <li key={subjectKey(subject)} className="space-y-1 py-2.5">
                  <p className="flex flex-wrap items-center gap-x-2 gap-y-1">
                    <SubjectLink subject={subject} className="min-w-0 truncate text-foreground" />
                    <SupervisionTag check={check} />
                  </p>
                  {next && <p className="text-[11px] text-muted-foreground">{next}</p>}
                  {status?.lastError && <DevhqRunError error={status.lastError} />}
                </li>
              )
            })}
          </ul>
        )}
        {unwatched.length > 0 && (
          <p className="leading-5 text-muted-foreground">
            Not supervised ({unwatched.length}):{' '}
            {unwatched.map(({ subject }, index) => (
              <Fragment key={subjectKey(subject)}>
                {index > 0 && ', '}
                <SubjectLink subject={subject} />
              </Fragment>
            ))}
            . Their evidence changes only when you record it or run a check.
          </p>
        )}
        {unavailable && (
          <p className="break-words text-rose-300">
            Supervision status is unavailable: {unavailable}
          </p>
        )}
        <p className="text-[11px] leading-5 text-muted-foreground">
          Supervision runs only while DevHQ is open. Closing it stops every check; missed runs are
          not made up.
        </p>
      </div>
    </section>
  )
}
