import { useEffect, useId, useState } from 'react'
import { Button } from '@/components/ui/button'
import { formatDateTime } from '@/shared/time'
import { cn } from '@/shared/utils'
import { useCopy } from './copy'
import { inventoryError, type AgentAccess, type AgentAccessMode } from './inventory-api'
import { useInventory } from './inventory-context'

const modes: { value: AgentAccessMode; label: string; detail: string }[] = [
  {
    value: 'off',
    label: 'Off',
    detail: 'Agents can only ask which inventory this is and how to open the gate.',
  },
  {
    value: 'read',
    label: 'Read only',
    detail:
      'Agents can read what needs attention, every application and resource, and their history. They change nothing.',
  },
  {
    value: 'maintain',
    label: 'Read and maintain',
    detail:
      'Agents can also add applications and resources and edit them, with their components and references. An edit only changes what it names, and is refused if someone changed the system after the agent read it. They can read local copies and record what they observed, such as a review of an application, tagged as reported by them and valid for 7 days at most. They cannot delete anything, record incidents or events, set up or run checks, or back up. Each change is tagged as theirs in Activity, with every field before and after.',
  },
  {
    value: 'write',
    label: 'Read and write',
    detail:
      'Agents can also do everything else you do here: record incidents and events, record observations valid for up to 30 days, set up and run checks, delete what has no history and back up. This is broad access: DevHQ does not ask you before any of it. Each change is tagged as theirs.',
  },
]

/** The command that registers the gate with Claude Code, once, for every later local session. */
const registerCommand = (executable: string) =>
  `claude mcp add --scope user devhq -- "${executable}" --mcp`

/**
 * Whether an agent the person works with, such as Claude Code, may act in DevHQ through the agent
 * gate (`devhq --mcp`), and how to connect it. Off until the person opens it; the gate reads the
 * choice on every call, so it applies at once, also to a session already running.
 */
export function AgentGate() {
  const { agentAccess, setAgentAccess } = useInventory()
  const name = useId()
  const [access, setAccess] = useState<AgentAccess | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [saving, setSaving] = useState<AgentAccessMode | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)
  const { copied, copy } = useCopy()
  useEffect(() => {
    let current = true
    agentAccess().then(
      (value) => current && setAccess(value),
      (cause) => current && setLoadError(inventoryError(cause)),
    )
    return () => {
      current = false
    }
  }, [agentAccess])

  const choose = async (mode: AgentAccessMode) => {
    if (saving) return
    setSaving(mode)
    setSaveError(null)
    try {
      setAccess(await setAgentAccess(mode))
    } catch (cause) {
      // The choice in force stays selected: nothing changed.
      setSaveError(inventoryError(cause))
    } finally {
      setSaving(null)
    }
  }

  return (
    <section className="panel max-w-3xl p-6" aria-labelledby="agent-gate-heading">
      <h2 id="agent-gate-heading" className="section-heading">
        Agent gate
      </h2>
      <p className="mt-4 text-xs leading-6 text-muted-foreground">
        An agent such as Claude Code can act in DevHQ for you through the agent gate, when you ask
        it to in a session. The agent starts the gate with its session and ends it with the session:
        nothing runs in the background, nothing listens on the network and no credentials are kept.
        Whatever an agent records is tagged with its name, among your own entries. While this window
        is open, what an agent changes shows here by itself within about 2 seconds.
      </p>
      {access ? (
        <>
          <fieldset className="mt-5" disabled={saving !== null}>
            <legend className="text-[13px] font-semibold">Access</legend>
            <div className="mt-3 space-y-2">
              {modes.map((mode) => (
                <label
                  key={mode.value}
                  className={cn(
                    'flex cursor-pointer gap-3 rounded-md border px-3 py-2.5 text-xs',
                    access.mode === mode.value
                      ? 'border-primary/50 bg-primary/5'
                      : 'bg-background/40',
                  )}
                >
                  <input
                    type="radio"
                    name={name}
                    value={mode.value}
                    checked={access.mode === mode.value}
                    onChange={() => void choose(mode.value)}
                    className="mt-0.5 accent-primary"
                  />
                  <span>
                    <span className="font-medium text-foreground">{mode.label}</span>
                    <span className="mt-0.5 block leading-5 text-muted-foreground">
                      {mode.detail}
                    </span>
                  </span>
                </label>
              ))}
            </div>
          </fieldset>
          <p className="mt-3 text-xs leading-6 text-muted-foreground">
            What you choose here is not an approval of each change. It holds until you change it,
            for every agent session that opens this inventory: Claude Code, Codex or any other
            client.
          </p>
          <p role="status" className="mt-2 text-[11px] text-muted-foreground">
            {saving
              ? 'Saving…'
              : access.changedAt
                ? `Changed ${formatDateTime(access.changedAt)}. It applies at once, also to a session already running.`
                : 'Never opened: agents cannot read or change anything.'}
          </p>
          {saveError && (
            <p role="alert" className="mt-2 text-xs text-rose-300">
              The access could not be changed. {saveError}
            </p>
          )}
          <div className="mt-6 border-t pt-5">
            <h3 className="text-[13px] font-semibold">Connect Claude Code</h3>
            {access.executable ? (
              <>
                <p className="mt-2 text-xs leading-6 text-muted-foreground">
                  Run this once in a terminal. Claude Code keeps it for your user, so every later
                  local session has DevHQ’s tools. DevHQ never runs it for you.
                </p>
                <p className="mt-3 select-all break-all rounded-md border bg-background/40 px-3 py-2 font-mono text-[11px] text-foreground/90">
                  {registerCommand(access.executable)}
                </p>
                <div className="mt-3 flex flex-wrap items-center gap-3">
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => void copy(registerCommand(access.executable!))}
                  >
                    Copy command
                  </Button>
                  <p role="status" className="text-[11px] text-muted-foreground">
                    {copied && (
                      <span key={copied.at} className="fade-in">
                        {copied.ok
                          ? 'Command copied.'
                          : 'The command could not be copied: select it and copy it.'}
                      </span>
                    )}
                  </p>
                </div>
                <p className="mt-3 text-xs leading-6 text-muted-foreground">
                  The Claude desktop app is a packaged app: on Windows, a gate it starts may see
                  another copy of this inventory. Its <span className="font-mono">status</span> tool
                  says which file it opened.
                </p>
              </>
            ) : (
              <p role="alert" className="mt-2 text-xs text-rose-300">
                DevHQ could not tell where its executable is, so it cannot give the command.
              </p>
            )}
          </div>
        </>
      ) : loadError ? (
        <p role="alert" className="mt-4 text-xs text-rose-300">
          DevHQ could not read whether agents may use the gate. {loadError}
        </p>
      ) : (
        <p role="status" className="mt-4 text-xs text-muted-foreground">
          Reading the agent gate’s access…
        </p>
      )}
    </section>
  )
}
