import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { lifecycleLabels } from '@/domain/labels'
import type { ComponentKind, Lifecycle } from '@/domain/models'
import { DeleteSubject } from './delete-subject'
import { inventoryError, type ApplicationInput, type InventoryApplication } from './inventory-api'
import { useInventory } from './inventory-context'
import { textLimits } from './limits'
import { ReferenceFields } from './reference-fields'
import { referenceInputs } from './references'

type DraftComponent = ApplicationInput['components'][number]

export function ApplicationForm({
  application,
  onDone,
  onDeleted = onDone,
}: {
  application?: InventoryApplication
  onDone: () => void
  /** After a successful deletion; the application no longer exists. */
  onDeleted?: () => void
}) {
  const { snapshot, saveApplication } = useInventory()
  const [name, setName] = useState(application?.name ?? '')
  const [description, setDescription] = useState(application?.description ?? '')
  const [lifecycle, setLifecycle] = useState<Lifecycle>(application?.lifecycle ?? 'development')
  const [version, setVersion] = useState(application?.version ?? '')
  const [components, setComponents] = useState<DraftComponent[]>(application?.components ?? [])
  const [references, setReferences] = useState<ApplicationInput['references']>(
    application?.references ?? [],
  )
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const updateComponent = (index: number, patch: Partial<DraftComponent>) =>
    setComponents((current) =>
      current.map((item, position) => (position === index ? { ...item, ...patch } : item)),
    )
  return (
    <form
      className="panel space-y-5 p-5"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        setError(null)
        try {
          await saveApplication({
            id: application?.id,
            name,
            description,
            lifecycle,
            version: version.trim() || null,
            components,
            references: referenceInputs(references),
          })
          onDone()
        } catch (cause) {
          setError(inventoryError(cause))
        } finally {
          setBusy(false)
        }
      }}
    >
      <h2 className="section-heading">{application ? 'Edit application' : 'New application'}</h2>
      <div className="grid gap-4 min-[1100px]:grid-cols-2">
        <label className="grid gap-1.5 text-xs">
          Name{' '}
          <Input
            required
            spellCheck={false}
            maxLength={textLimits.name}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <label className="grid gap-1.5 text-xs">
          Lifecycle
          <select
            className="filter-select block w-full"
            value={lifecycle}
            onChange={(event) => setLifecycle(event.target.value as Lifecycle)}
          >
            {Object.entries(lifecycleLabels).map(([value, label]) => (
              <option value={value} key={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label className="grid gap-1.5 text-xs">
          Description{' '}
          <Input
            maxLength={textLimits.description}
            value={description}
            onChange={(event) => setDescription(event.target.value)}
          />
        </label>
        <label className="grid gap-1.5 text-xs">
          Version{' '}
          <Input
            spellCheck={false}
            maxLength={textLimits.version}
            value={version}
            onChange={(event) => setVersion(event.target.value)}
          />
        </label>
      </div>
      <div className="space-y-3 border-t pt-5">
        <div className="flex items-center justify-between gap-3">
          <h3 className="section-heading">Components</h3>
          <Button
            type="button"
            variant="outline"
            onClick={() =>
              setComponents((current) => [
                ...current,
                { name: '', kind: 'backend', resourceId: null },
              ])
            }
          >
            Add component
          </Button>
        </div>
        {components.length === 0 && (
          <p className="text-xs text-muted-foreground">
            Add a component to connect this application to a resource.
          </p>
        )}
        {components.map((component, index) => (
          <div
            className="grid gap-3 rounded-lg border p-3 min-[1100px]:grid-cols-[1fr_140px_1fr_auto]"
            key={component.id ?? `new-${index}`}
          >
            <label className="grid gap-1.5 text-xs">
              Component name{' '}
              <Input
                required
                spellCheck={false}
                maxLength={textLimits.name}
                value={component.name}
                onChange={(event) => updateComponent(index, { name: event.target.value })}
              />
            </label>
            <label className="grid gap-1.5 text-xs">
              Kind
              <select
                className="filter-select block w-full"
                value={component.kind}
                onChange={(event) =>
                  updateComponent(index, { kind: event.target.value as ComponentKind })
                }
              >
                {['backend', 'database', 'docker', 'frontend'].map((kind) => (
                  <option key={kind}>{kind}</option>
                ))}
              </select>
            </label>
            <label className="grid gap-1.5 text-xs">
              Resource
              <select
                className="filter-select block w-full"
                value={component.resourceId ?? ''}
                onChange={(event) =>
                  updateComponent(index, { resourceId: event.target.value || null })
                }
              >
                <option value="">Unlinked · not observed</option>
                {snapshot?.resources.map((resource) => (
                  <option value={resource.id} key={resource.id}>
                    {resource.name}
                  </option>
                ))}
              </select>
            </label>
            <Button
              type="button"
              variant="ghost"
              className="self-end"
              aria-label={`Remove ${component.name || 'component'}`}
              onClick={() =>
                setComponents((current) => current.filter((_, position) => position !== index))
              }
            >
              Remove
            </Button>
          </div>
        ))}
      </div>
      <ReferenceFields references={references} onChange={setReferences} />
      {error && (
        <p role="alert" className="text-xs text-rose-300">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="submit" disabled={busy}>
          {busy ? 'Saving…' : 'Save application'}
        </Button>
        <Button type="button" variant="outline" onClick={onDone}>
          Cancel
        </Button>
      </div>
      {application && (
        <DeleteSubject
          subject={{ type: 'application', id: application.id }}
          name={application.name}
          onDeleted={onDeleted}
        />
      )}
    </form>
  )
}
