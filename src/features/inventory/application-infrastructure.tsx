import { ArrowUpRight, Server, Unlink } from 'lucide-react'
import { Link } from 'react-router-dom'
import { EmptyState } from '@/components/empty-state'
import { sameSubject, subjectEvidence } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import { activeIncidents } from '@/domain/selectors'
import { EvidenceBadge, EvidenceDetail, ObservationHistoryToggle } from './evidence'
import { SupervisionTag } from './http-check'
import type { InventoryApplication, InventoryComponent } from './inventory-api'
import { useInventory } from './inventory-context'
import { IncidentRecordList } from './record-lists'

const componentList = (components: InventoryComponent[]) =>
  components.map((part) => `${part.name} (${part.kind})`).join(', ')

/**
 * The resources an application runs on, each with its own evidence, check and incidents, and
 * the other applications that share it: a problem there concerns all of them.
 */
export function ApplicationInfrastructure({ app }: { app: InventoryApplication }) {
  const { snapshot, now } = useInventory()
  if (!snapshot) return null
  const linked = [
    ...new Set(app.components.flatMap((part) => (part.resourceId ? [part.resourceId] : []))),
  ].flatMap((id) => {
    const resource = snapshot.resources.find((item) => item.id === id)
    return resource ? [resource] : []
  })
  const unlinked = app.components.filter((part) => !part.resourceId)
  if (!app.components.length)
    return (
      <EmptyState
        title="No components registered"
        description="Add components in Edit application and link them to the resources they run on. Until then DevHQ cannot relate this application to its infrastructure."
      />
    )
  const incidents = activeIncidents(snapshot.incidents)
  return (
    <div className="space-y-5">
      <p className="text-xs leading-5 text-muted-foreground">
        The resources {app.name} runs on, each with its own evidence. Their incidents and activity
        also appear in this application; a problem on a shared resource concerns every application
        that uses it.
      </p>
      {linked.map((resource) => {
        const subject: EntityRef = { type: 'resource', id: resource.id }
        const evidence = subjectEvidence(snapshot.observations, subject, now)
        const check = snapshot.httpChecks.find((item) => sameSubject(item.subject, subject))
        const open = incidents.filter((item) => sameSubject(item.subject, subject))
        const here = app.components.filter((part) => part.resourceId === resource.id)
        const sharedWith = snapshot.applications.filter(
          (other) =>
            other.id !== app.id && other.components.some((part) => part.resourceId === resource.id),
        )
        return (
          <section key={resource.id} aria-label={resource.name} className="panel p-5">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div className="flex min-w-0 gap-3">
                <Server
                  className="mt-0.5 size-4 shrink-0 text-muted-foreground"
                  aria-hidden="true"
                />
                <div className="min-w-0">
                  <Link
                    className="quiet-link text-base font-semibold text-foreground"
                    to={`/infrastructure?resource=${resource.id}`}
                  >
                    {resource.name} <ArrowUpRight className="size-3.5" />
                  </Link>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {resource.kind === 'server' ? 'Server' : 'Workstation'} ·{' '}
                    {resource.operatingSystem || 'Operating system not recorded'}
                  </p>
                </div>
              </div>
              <EvidenceBadge state={evidence.state} />
            </div>
            <dl className="mt-4 grid gap-x-6 gap-y-3 text-xs min-[1100px]:grid-cols-[160px_1fr]">
              <dt className="text-muted-foreground">Runs here</dt>
              <dd>{componentList(here)}</dd>
              <dt className="text-muted-foreground">HTTP check</dt>
              <dd className="flex flex-wrap items-center gap-2">
                {check ? (
                  <>
                    <span className="break-all font-mono text-[11px] text-foreground/85">
                      GET {check.url}
                    </span>
                    <SupervisionTag check={check} />
                  </>
                ) : (
                  <span className="text-muted-foreground">
                    Not configured: its evidence changes only when you record it.
                  </span>
                )}
              </dd>
              <dt className="text-muted-foreground">Shared with</dt>
              <dd>
                {sharedWith.length ? (
                  <span className="flex flex-wrap gap-3">
                    {sharedWith.map((other) => (
                      <Link className="quiet-link" to={`/applications/${other.id}`} key={other.id}>
                        {other.name} <ArrowUpRight className="size-3" />
                      </Link>
                    ))}
                  </span>
                ) : (
                  <span className="text-muted-foreground">No other application uses it</span>
                )}
              </dd>
            </dl>
            <div className="mt-4 border-t pt-4">
              <h3 className="eyebrow">Latest evidence</h3>
              <EvidenceDetail evidence={evidence} now={now} />
              {evidence.latest && (
                <div className="mt-3">
                  <ObservationHistoryToggle subject={subject} name={resource.name} />
                </div>
              )}
            </div>
            {open.length > 0 && (
              <div className="-mx-5 mt-4 border-t">
                <h3 className="eyebrow px-5 pt-4">Open resource incidents</h3>
                <IncidentRecordList items={open} />
              </div>
            )}
          </section>
        )
      })}
      {unlinked.length > 0 && (
        <section
          aria-label="Components without a resource"
          className="flex gap-3 rounded-xl border border-dashed p-5"
        >
          <Unlink className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <div className="text-xs leading-5">
            <h3 className="font-medium">Not linked to a resource</h3>
            <p className="mt-1 text-muted-foreground">
              {componentList(unlinked)}. DevHQ cannot relate their infrastructure or show its
              evidence; link them in Edit application.
            </p>
          </div>
        </section>
      )}
    </div>
  )
}
