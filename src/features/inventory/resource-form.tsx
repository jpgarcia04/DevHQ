import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { DeleteSubject } from './delete-subject'
import { inventoryError, type InventoryResource, type ResourceInput } from './inventory-api'
import { useInventory } from './inventory-context'
import { textLimits } from './limits'
import { ReferenceFields } from './reference-fields'
import { referenceInputs } from './references'

export function ResourceForm({
  resource,
  onDone,
}: {
  resource?: InventoryResource
  onDone: () => void
}) {
  const { saveResource } = useInventory()
  const [name, setName] = useState(resource?.name ?? '')
  const [kind, setKind] = useState<ResourceInput['kind']>(resource?.kind ?? 'server')
  const [description, setDescription] = useState(resource?.description ?? '')
  const [operatingSystem, setOperatingSystem] = useState(resource?.operatingSystem ?? '')
  const [references, setReferences] = useState<ResourceInput['references']>(
    resource?.references ?? [],
  )
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  return (
    <form
      className="panel space-y-4 p-5"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        setError(null)
        try {
          await saveResource({
            id: resource?.id,
            name,
            kind,
            description,
            operatingSystem,
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
      <h2 className="section-heading">{resource ? 'Edit resource' : 'New resource'}</h2>
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
          Kind
          <select
            className="filter-select block w-full"
            value={kind}
            onChange={(event) => setKind(event.target.value as ResourceInput['kind'])}
          >
            <option value="server">Server</option>
            <option value="workstation">Workstation</option>
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
          Operating system{' '}
          <Input
            spellCheck={false}
            maxLength={textLimits.name}
            value={operatingSystem}
            onChange={(event) => setOperatingSystem(event.target.value)}
          />
        </label>
      </div>
      {/* Once it exists, a resource can be the machine of its own paths. */}
      <ReferenceFields
        references={references}
        onChange={setReferences}
        self={resource && { id: resource.id, kind }}
      />
      {error && (
        <p role="alert" className="text-xs text-rose-300">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button type="submit" disabled={busy}>
          {busy ? 'Saving…' : 'Save resource'}
        </Button>
        <Button type="button" variant="outline" onClick={onDone}>
          Cancel
        </Button>
      </div>
      {resource && (
        <DeleteSubject
          subject={{ type: 'resource', id: resource.id }}
          name={resource.name}
          onDeleted={onDone}
        />
      )}
    </form>
  )
}
