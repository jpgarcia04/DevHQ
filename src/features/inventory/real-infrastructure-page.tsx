import { useEffect, useRef, useState } from 'react'
import { ArrowUpRight, Pencil, Plus } from 'lucide-react'
import { Link, useLocation, useSearchParams } from 'react-router-dom'
import { EmptyState } from '@/components/empty-state'
import { PageHeader } from '@/components/page-header'
import { Button } from '@/components/ui/button'
import { subjectEvidence } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import { activeIncidents } from '@/domain/selectors'
import { EvidenceBadge, EvidenceDetail, ObservationHistoryToggle } from './evidence'
import { HttpCheckPanel } from './http-check'
import { useInventory } from './inventory-context'
import { InventoryState } from './inventory-state'
import { RecordActions } from './record-forms'
import { IncidentRecordList } from './record-lists'
import { ResourceForm } from './resource-form'

export function RealInfrastructurePage() {
  const { snapshot, now } = useInventory()
  const [params] = useSearchParams()
  const { key: navigation } = useLocation()
  const selectedResource = params.get('resource')
  const [editing, setEditing] = useState<string | 'new' | null>(null)
  const focusedFor = useRef<string | null>(null)
  useEffect(() => {
    // Once per navigation, as soon as the linked resource is shown. A later snapshot (a save,
    // a supervised run) must not pull focus away and scroll back to it.
    if (!selectedResource || focusedFor.current === navigation) return
    const timeout = window.setTimeout(() => {
      const element = document.getElementById(`resource-${selectedResource}`)
      if (!element) return
      focusedFor.current = navigation
      element.focus({ preventScroll: true })
      element.scrollIntoView?.({ block: 'start' })
    }, 0)
    return () => window.clearTimeout(timeout)
  }, [navigation, selectedResource, snapshot])
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory"
        title="Infrastructure"
        description="Resources shared by the components in your applications."
        action={
          <Button onClick={() => setEditing('new')}>
            <Plus className="size-4" /> New resource
          </Button>
        }
      />
      <InventoryState>
        {editing === 'new' && <ResourceForm onDone={() => setEditing(null)} />}
        {snapshot?.resources.length ? (
          <div className="space-y-5">
            {snapshot.resources.map((resource) => {
              const subject: EntityRef = { type: 'resource', id: resource.id }
              const evidence = subjectEvidence(snapshot.observations, subject, now)
              const incidents = activeIncidents(snapshot.incidents).filter(
                (item) => item.subject.type === 'resource' && item.subject.id === resource.id,
              )
              const linked = snapshot.applications.filter((app) =>
                app.components.some((part) => part.resourceId === resource.id),
              )
              return (
                <section
                  className={`panel p-5 ${params.get('resource') === resource.id ? 'border-primary/50' : ''}`}
                  id={`resource-${resource.id}`}
                  tabIndex={-1}
                  aria-label={resource.name}
                  key={resource.id}
                >
                  <div className="flex flex-wrap items-start justify-between gap-4">
                    <div>
                      <h2 className="text-base font-semibold">{resource.name}</h2>
                      <p className="mt-1 text-xs text-muted-foreground">
                        {resource.description || resource.kind} ·{' '}
                        {resource.operatingSystem || 'Operating system not recorded'}
                      </p>
                    </div>
                    <div className="flex items-center gap-3">
                      <EvidenceBadge state={evidence.state} />
                      <Button variant="outline" onClick={() => setEditing(resource.id)}>
                        <Pencil className="size-3.5" /> Edit
                      </Button>
                    </div>
                  </div>
                  <EvidenceDetail evidence={evidence} now={now} />
                  {evidence.latest && (
                    <div className="mt-3">
                      <ObservationHistoryToggle subject={subject} name={resource.name} />
                    </div>
                  )}
                  <div className="mt-4 border-t pt-4">
                    <HttpCheckPanel subject={subject} name={resource.name} />
                  </div>
                  <div className="mt-4 border-t pt-4">
                    <h3 className="eyebrow mb-3">Applications using this resource</h3>
                    {linked.length ? (
                      <div className="flex flex-wrap gap-3">
                        {linked.map((app) => (
                          <Link className="quiet-link" to={`/applications/${app.id}`} key={app.id}>
                            {app.name} <ArrowUpRight className="size-3" />
                          </Link>
                        ))}
                      </div>
                    ) : (
                      <p className="text-xs text-muted-foreground">No linked components.</p>
                    )}
                  </div>
                  {incidents.length > 0 && (
                    <div className="-mx-5 mt-4 border-t">
                      <h3 className="eyebrow px-5 pt-4">Open resource incidents</h3>
                      <IncidentRecordList items={incidents} />
                    </div>
                  )}
                  <div className="mt-4 border-t pt-4">
                    <RecordActions subject={subject} name={resource.name} />
                  </div>
                  {editing === resource.id && (
                    <div className="mt-5">
                      <ResourceForm resource={resource} onDone={() => setEditing(null)} />
                    </div>
                  )}
                </section>
              )
            })}
          </div>
        ) : (
          <EmptyState
            title="No resources registered"
            description="Add a resource, then link an application component to it."
            action={<Button onClick={() => setEditing('new')}>New resource</Button>}
          />
        )}
      </InventoryState>
    </div>
  )
}
