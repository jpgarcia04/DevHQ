import { useEffect, useState } from 'react'
import { PageHeader } from '@/components/page-header'
import { Button } from '@/components/ui/button'
import { inventoryError, type DataLocation } from './inventory-api'
import { useInventory } from './inventory-context'

/**
 * The inventory's file on this device and how to back it up. It asks the native process only
 * where the file is, so it still answers when the inventory cannot be read: then above all.
 */
function DatabaseFile() {
  const { dataLocation } = useInventory()
  const [location, setLocation] = useState<DataLocation | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState<boolean | null>(null)
  useEffect(() => {
    let current = true
    dataLocation().then(
      (value) => current && setLocation(value),
      (cause) => current && setError(inventoryError(cause)),
    )
    return () => {
      current = false
    }
  }, [dataLocation])
  const copy = async (path: string) => {
    try {
      await navigator.clipboard.writeText(path)
      setCopied(true)
    } catch {
      setCopied(false)
    }
  }
  return (
    <section className="panel max-w-3xl p-6" aria-labelledby="database-file-heading">
      <h2 id="database-file-heading" className="section-heading">
        Database file
      </h2>
      {location ? (
        <>
          <p className="mt-4 select-all break-all rounded-md border bg-background/40 px-3 py-2 font-mono text-[11px] text-foreground/90">
            {location.database}
          </p>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <Button size="sm" variant="outline" onClick={() => void copy(location.database)}>
              Copy path
            </Button>
            <p role="status" className="text-[11px] text-muted-foreground">
              {copied === true && 'Path copied.'}
              {copied === false && 'The path could not be copied: select it and copy it.'}
            </p>
          </div>
          <p className="mt-4 text-xs leading-6 text-muted-foreground">
            To back it up, close DevHQ and copy this file. While DevHQ is open, its latest changes
            can still be in{' '}
            <span className="whitespace-nowrap font-mono">inventory.sqlite3-wal</span> in the same
            folder; DevHQ folds them into the file when it closes.
          </p>
          <p className="mt-3 text-xs leading-6 text-muted-foreground">
            To restore a copy, close DevHQ, put the copy in place of this file and delete{' '}
            <span className="whitespace-nowrap font-mono">inventory.sqlite3-wal</span> and{' '}
            <span className="whitespace-nowrap font-mono">inventory.sqlite3-shm</span> if they are
            there, so that no log of the replaced file is applied to the copy.
          </p>
        </>
      ) : error ? (
        <p role="alert" className="mt-4 text-xs text-rose-300">
          DevHQ could not say where its database file is. {error}
        </p>
      ) : (
        <p role="status" className="mt-4 text-xs text-muted-foreground">
          Locating the database file…
        </p>
      )}
    </section>
  )
}

export function RealSettingsPage() {
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory"
        title="Settings"
        description="Your data is stored locally on this device."
      />
      <DatabaseFile />
      <section className="panel max-w-3xl p-6">
        <h2 className="section-heading">Data source</h2>
        <p className="mt-4 text-xs leading-6 text-muted-foreground">
          Applications, components, resources, observations, incidents and events are stored in
          SQLite in the operating system’s DevHQ application data directory. The demo is a separate,
          fixed example and is never imported automatically.
        </p>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          Observations come from two sources, always labelled: manual entries you type, and HTTP
          checks that DevHQ runs when you press Run check or, if you turn supervision on, on a
          schedule while DevHQ is open. There is no background service, retry, event collection or
          automation execution. Incidents and events are manual entries. An observation vouches for
          a result only until its validity ends; after that it is shown as expired, whatever its
          source.
        </p>
      </section>
      <section className="panel max-w-3xl p-6">
        <h2 className="section-heading">HTTP checks</h2>
        <p className="mt-4 text-xs leading-6 text-muted-foreground">
          Each application or resource can have one check: a single GET sent by DevHQ’s native
          process, never by this window, straight to the URL without a proxy. Certificates are
          verified by the operating system, at most 5 redirects are followed and recorded, and the
          whole request gives up after at most 10 seconds. No credentials, custom headers or body
          are sent. Only the status, timing, redirects and error are stored, never the response body
          or headers.
        </p>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          If the checked system fails (DNS, refused connection, TLS, timeout or an unexpected
          status), that is evidence and is recorded as Unhealthy. If DevHQ fails (invalid or missing
          configuration, database), you see an error and nothing is recorded.
        </p>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          Supervision runs a check every 5 to 60 minutes, only while this window is open: closing
          DevHQ stops it, and nothing runs in the background or with the app closed. A check never
          runs twice at once and failures are not retried; the next run comes one interval after the
          last one ended. When DevHQ opens again, an overdue check runs once, not once per missed
          interval. Each result must stay valid for longer than the interval, so supervised evidence
          does not lapse between runs. Pausing lets a run in progress finish; removing a check
          cancels it and records nothing.
        </p>
      </section>
    </div>
  )
}
