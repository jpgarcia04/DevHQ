import { useState, type FormEvent } from 'react'
import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { formatDateTime } from '@/shared/time'
import { SubjectLink } from './evidence'
import { inventoryError } from './inventory-api'
import { useInventory } from './inventory-context'

/**
 * Which workstation this computer is: what lets DevHQ read local copies here and nowhere else. It
 * is kept on this computer, keyed by a fingerprint of the machine, never in the inventory, so an
 * inventory or a home folder copied to another computer does not make that one take itself for
 * this one. Only this window chooses it; the agent gate only reads it.
 */
export function ThisComputer() {
  const { snapshot, thisWorkstation, workstationError, setThisWorkstation } = useInventory()
  // Untouched, the select shows the current choice, which arrives apart from the inventory.
  const [draft, setDraft] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [outcome, setOutcome] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const workstations = snapshot?.resources.filter((item) => item.kind === 'workstation') ?? []
  const current = thisWorkstation?.resourceId ?? ''
  const value = draft ?? current
  const save = async (event: FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setOutcome(null)
    setError(null)
    try {
      const answer = await setThisWorkstation(value || null)
      setDraft(null)
      setOutcome(
        answer.resourceId
          ? 'Saved. DevHQ reads the local copies declared on this workstation when you ask.'
          : 'Forgotten. DevHQ reads no local copy until you choose again.',
      )
    } catch (cause) {
      setError(inventoryError(cause))
    } finally {
      setBusy(false)
    }
  }
  const file = thisWorkstation && (thisWorkstation.storedAt ?? thisWorkstation.file)
  return (
    <section className="panel max-w-3xl p-6" aria-labelledby="this-computer-heading">
      <h2 id="this-computer-heading" className="section-heading">
        This computer
      </h2>
      <p className="mt-4 text-xs leading-6 text-muted-foreground">
        DevHQ reads a local copy only on the workstation where it is declared, so it must know which
        one this computer is. That stays on this computer, beside a fingerprint of the machine, and
        never in the inventory: another computer with a copy of the inventory or of your home folder
        has not chosen yet. Choosing is not a change to the inventory and is not in Activity; each
        reading keeps the machine it was made on. Agents can see the choice, never make it.
      </p>
      <div className="mt-4 text-xs leading-6">
        {thisWorkstation?.resourceId ? (
          <p>
            This computer is{' '}
            <SubjectLink
              subject={{ type: 'resource', id: thisWorkstation.resourceId }}
              className="text-foreground"
            />
            {thisWorkstation.chosenAt && (
              <span className="text-muted-foreground">
                , chosen{' '}
                <time dateTime={thisWorkstation.chosenAt}>
                  {formatDateTime(thisWorkstation.chosenAt)}
                </time>
              </span>
            )}
            .
          </p>
        ) : thisWorkstation ? (
          <p className="text-amber-200">
            DevHQ does not know which workstation this computer is. {thisWorkstation.message} It
            reads no local copy until it does.
          </p>
        ) : workstationError ? (
          <p role="alert" className="text-rose-300">
            DevHQ could not say which workstation this computer is. {workstationError}
          </p>
        ) : (
          <p role="status" className="text-muted-foreground">
            Asking which workstation this computer is…
          </p>
        )}
      </div>
      {thisWorkstation?.problem !== 'no_machine_id' &&
        thisWorkstation?.problem !== 'unreadable' &&
        (workstations.length ? (
          <form
            className="mt-4 flex flex-wrap items-end gap-3"
            aria-label="Choose this computer"
            onSubmit={(event) => void save(event)}
          >
            <label className="grid gap-1.5 text-xs">
              Workstation
              <select
                className="filter-select block min-w-56"
                value={value}
                onChange={(event) => setDraft(event.target.value)}
              >
                <option value="">None chosen</option>
                {workstations.map((item) => (
                  <option value={item.id} key={item.id}>
                    {item.name}
                  </option>
                ))}
              </select>
            </label>
            <Button type="submit" size="sm" disabled={busy || value === current}>
              {busy ? 'Saving…' : 'Save'}
            </Button>
          </form>
        ) : (
          snapshot && (
            <p className="mt-4 text-xs leading-6 text-muted-foreground">
              No workstation is registered. Add this computer in{' '}
              <Link className="text-primary hover:underline" to="/infrastructure">
                Infrastructure
              </Link>{' '}
              as a resource of kind Workstation, then choose it here.
            </p>
          )
        ))}
      <p role="status" className="mt-3 text-[11px] text-muted-foreground empty:hidden">
        {outcome}
      </p>
      {error && (
        <p role="alert" className="mt-3 text-xs break-words text-rose-300">
          {error}
        </p>
      )}
      {file && (
        <>
          <p className="mt-4 text-[11px] text-muted-foreground">Kept in</p>
          <p className="mt-1 rounded-md border bg-background/40 px-3 py-2 font-mono text-[11px] break-all text-foreground/90 select-all">
            {file}
          </p>
          {thisWorkstation.storedAt && (
            <p className="mt-3 rounded-md border border-amber-400/20 bg-amber-300/5 px-3 py-2 text-xs leading-6 text-amber-200">
              DevHQ asked for{' '}
              <span className="font-mono text-[11px] break-all">{thisWorkstation.file}</span>, but
              the file system keeps the file at the path above, as it does when DevHQ is started
              from a packaged app. DevHQ opened from the Start menu reads the other path, where this
              computer may not be chosen.
            </p>
          )}
          <p className="mt-3 text-[11px] leading-5 text-muted-foreground">
            The file holds, for each computer, a fingerprint and the workstation chosen in each
            inventory; never the machine’s own identifier. Back up now does not copy it.
          </p>
        </>
      )}
    </section>
  )
}
