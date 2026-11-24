import { useEffect, useId, useState, type FormEvent } from 'react'
import { Button } from '@/components/ui/button'
import { inventoryError, type CredentialStore, type GithubToken } from './inventory-api'
import { useInventory } from './inventory-context'

const storeNames: Record<CredentialStore, string> = {
  windows_credential_manager: 'Windows Credential Manager',
  linux_session_keyring: 'the Linux kernel keyring of your login session',
  none: 'no credential store',
}

/**
 * The fine-grained GitHub token DevHQ reads the repositories the inventory names with
 * (docs/leer-github.md). The window sends it once, to the system's credential store, and only ever
 * learns whether one is kept: the field empties as soon as it is saved, and nothing shows it again.
 */
export function GithubTokenSettings() {
  const { githubToken, saveGithubToken, removeGithubToken } = useInventory()
  const field = useId()
  const [token, setToken] = useState<GithubToken | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [text, setText] = useState('')
  const [busy, setBusy] = useState<'save' | 'remove' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [done, setDone] = useState<string | null>(null)
  useEffect(() => {
    let current = true
    githubToken().then(
      (value) => current && setToken(value),
      (cause) => current && setLoadError(inventoryError(cause)),
    )
    return () => {
      current = false
    }
  }, [githubToken])

  const run = async (kind: 'save' | 'remove', action: () => Promise<GithubToken>) => {
    if (busy) return
    setBusy(kind)
    setError(null)
    setDone(null)
    try {
      setToken(await action())
      setLoadError(null)
      setDone(kind === 'save' ? 'Token saved.' : 'Token removed.')
      if (kind === 'save') setText('')
    } catch (cause) {
      // What was kept before stays kept: a refusal changes nothing.
      setError(inventoryError(cause))
    } finally {
      setBusy(null)
    }
  }

  const save = (event: FormEvent) => {
    event.preventDefault()
    if (text.trim()) void run('save', () => saveGithubToken(text))
  }

  const where = token ? storeNames[token.store] : 'the system’s credential store'
  return (
    <section className="panel max-w-3xl p-6" aria-labelledby="github-token-heading">
      <h2 id="github-token-heading" className="section-heading">
        GitHub token
      </h2>
      <p className="mt-4 text-xs leading-6 text-muted-foreground">
        With this token DevHQ reads the GitHub repositories your applications and resources name as
        their repository: whether the default branch passes its checks and which pull requests have
        waited for days. It reads them once when it opens and when you press Read GitHub in Pending
        work, never in the background.
      </p>
      <p className="mt-3 text-xs leading-6 text-muted-foreground">
        On GitHub, create a fine-grained token (Settings › Developer settings › Personal access
        tokens › Fine-grained tokens) with access to only the repositories DevHQ should read, and
        read-only access to Metadata, Actions and Pull requests: with it, nothing can be changed.
        DevHQ hands it to {where} and never shows it again. It never goes into the inventory, a
        backup or the agent gate.
        {token?.store === 'linux_session_keyring' &&
          ' There it lasts until you log out or the computer restarts.'}
      </p>

      {loadError ? (
        <p role="alert" className="mt-4 text-xs text-rose-300">
          DevHQ could not say whether a token is kept. {loadError}
        </p>
      ) : token ? (
        <p role="status" className="mt-4 text-xs text-foreground/90">
          {token.store === 'none'
            ? 'This system has no credential store DevHQ knows, so it keeps no token here.'
            : token.stored
              ? `A token is kept in ${storeNames[token.store]}.`
              : 'No token is kept.'}
        </p>
      ) : (
        <p role="status" className="mt-4 text-xs text-muted-foreground">
          Asking the credential store…
        </p>
      )}

      {token?.store !== 'none' && (
        <form className="mt-4 flex flex-wrap items-end gap-3" onSubmit={save}>
          <div className="min-w-0 flex-1">
            <label htmlFor={field} className="text-[11px] text-muted-foreground">
              {token?.stored ? 'Replace it with another fine-grained token' : 'Fine-grained token'}
            </label>
            <input
              id={field}
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={text}
              onChange={(event) => setText(event.target.value)}
              placeholder="github_pat_…"
              className="mt-1 w-full rounded-md border bg-background/40 px-3 py-2 font-mono text-[11px]"
            />
          </div>
          <Button type="submit" size="sm" disabled={!text.trim() || busy !== null}>
            {busy === 'save' ? 'Saving…' : 'Save token'}
          </Button>
          {token?.stored && (
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy !== null}
              onClick={() => void run('remove', removeGithubToken)}
            >
              {busy === 'remove' ? 'Removing…' : 'Remove token'}
            </Button>
          )}
        </form>
      )}
      {error && (
        <p role="alert" className="mt-3 text-xs text-rose-300">
          {error}
        </p>
      )}
      {done && (
        <p role="status" className="mt-3 text-[11px] text-muted-foreground">
          {done}
        </p>
      )}
    </section>
  )
}
