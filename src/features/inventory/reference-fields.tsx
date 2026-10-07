import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { referenceKindLabels } from '@/domain/labels'
import type { ReferenceKind } from '@/domain/models'
import type { InventoryResource, ReferenceInput } from './inventory-api'
import { useInventory } from './inventory-context'
import { textLimits } from './limits'
import { isWebAddress } from './references'

const placeholders: Record<ReferenceKind, string> = {
  repository: 'https://…',
  local_copy: 'C:\\… or /…',
  documentation: 'https://…, C:\\… or /…',
  logs: 'https://…, C:\\… or /…',
  dashboard: 'https://…',
}

/**
 * The references of an application or resource in its form: the complete list, saved with it.
 * `self` is the resource whose form this is, once it exists: it can be the machine of its own
 * paths.
 */
export function ReferenceFields({
  references,
  onChange,
  self,
}: {
  references: ReferenceInput[]
  onChange: (references: ReferenceInput[]) => void
  self?: Pick<InventoryResource, 'id' | 'kind'>
}) {
  const { snapshot } = useInventory()
  const machines = (snapshot?.resources ?? []).filter((resource) => resource.id !== self?.id)
  const update = (index: number, patch: Partial<ReferenceInput>) =>
    onChange(
      references.map((item, position) => (position === index ? { ...item, ...patch } : item)),
    )
  return (
    <div className="space-y-3 border-t pt-5">
      <div className="flex items-center justify-between gap-3">
        <h3 className="section-heading">References</h3>
        <Button
          type="button"
          variant="outline"
          onClick={() =>
            onChange([
              ...references,
              { kind: 'repository', target: '', label: null, machineId: null },
            ])
          }
        >
          Add reference
        </Button>
      </div>
      {references.length === 0 && (
        <p className="text-xs text-muted-foreground">
          Add where its repository, local copies, documentation, logs or dashboard are. DevHQ keeps
          and copies them; it opens none.
        </p>
      )}
      {references.map((reference, index) => {
        const web = isWebAddress(reference.target)
        return (
          <div
            className="grid gap-3 rounded-lg border p-3 min-[1100px]:grid-cols-[150px_1fr_210px_150px_auto]"
            key={reference.id ?? `new-${index}`}
          >
            <label className="grid gap-1.5 text-xs">
              Reference kind
              <select
                className="filter-select block w-full"
                value={reference.kind}
                onChange={(event) => update(index, { kind: event.target.value as ReferenceKind })}
              >
                {Object.entries(referenceKindLabels).map(([value, label]) => (
                  <option value={value} key={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label className="grid gap-1.5 text-xs">
              URL or path{' '}
              <Input
                required
                spellCheck={false}
                maxLength={textLimits.url}
                placeholder={placeholders[reference.kind]}
                value={reference.target}
                onChange={(event) => update(index, { target: event.target.value })}
              />
            </label>
            <label className="grid gap-1.5 text-xs">
              Machine
              <select
                className="filter-select block w-full"
                disabled={web}
                value={web ? '' : (reference.machineId ?? '')}
                onChange={(event) => update(index, { machineId: event.target.value || null })}
              >
                <option value="">{web ? 'None · a URL' : 'Choose where the path is'}</option>
                {self && <option value={self.id}>This resource · {self.kind}</option>}
                {machines.map((resource) => (
                  <option value={resource.id} key={resource.id}>
                    {resource.name} · {resource.kind}
                  </option>
                ))}
              </select>
            </label>
            <label className="grid gap-1.5 text-xs">
              Label{' '}
              <Input
                maxLength={textLimits.name}
                placeholder="Optional name"
                value={reference.label ?? ''}
                onChange={(event) => update(index, { label: event.target.value })}
              />
            </label>
            <Button
              type="button"
              variant="ghost"
              className="self-end"
              aria-label={`Remove ${referenceKindLabels[reference.kind].toLowerCase()} ${reference.target || 'reference'}`}
              onClick={() => onChange(references.filter((_, position) => position !== index))}
            >
              Remove
            </Button>
          </div>
        )
      })}
    </div>
  )
}
