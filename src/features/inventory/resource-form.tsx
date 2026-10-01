import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { inventoryError, type InventoryResource, type ResourceInput } from './inventory-api'
import { useInventory } from './inventory-context'

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
          await saveResource({ id: resource?.id, name, kind, description, operatingSystem })
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
        <label className="space-y-1.5 text-xs">
          Name{' '}
          <Input
            required
            maxLength={120}
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <label className="space-y-1.5 text-xs">
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
        <label className="space-y-1.5 text-xs">
          Description{' '}
          <Input
            maxLength={1000}
            value={description}
            onChange={(event) => setDescription(event.target.value)}
          />
        </label>
        <label className="space-y-1.5 text-xs">
          Operating system{' '}
          <Input
            maxLength={120}
            value={operatingSystem}
            onChange={(event) => setOperatingSystem(event.target.value)}
          />
        </label>
      </div>
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
    </form>
  )
}
