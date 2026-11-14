import { useEffect, useId, useState } from 'react'
import { ChevronDown, PackageSearch, ScanSearch } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import type { EntityRef } from '@/domain/models'
import { counted } from '@/shared/count'
import { formatDateTime, relativeTime } from '@/shared/time'
import { cn } from '@/shared/utils'
import { MachineName } from './copy-readings'
import {
  buildText,
  declaredVersion,
  installationSummary,
  installedOnThisComputer,
  processesText,
  sizeText,
  versionText,
} from './installations'
import { inventoryError, type Installation, type InstallationReading } from './inventory-api'
import { useInventory } from './inventory-context'

/** Readings a page holds, as the native process pages them (`READINGS_PAGE`). */
const readingsPage = 200

const outcomeLabels: Record<InstallationReading['outcome'], string> = {
  read: 'Read',
  missing: 'Not found',
  unreadable: 'Not readable',
  not_an_executable: 'Not an executable',
}

function Moment({ value, now }: { value: string; now: number }) {
  return (
    <>
      <time dateTime={value}>{formatDateTime(value)}</time> (
      {relativeTime(value, now).toLowerCase()})
    </>
  )
}

/**
 * A reading's provenance: DevHQ read the file itself, with the reader named in the tooltip, and an
 * agent's name when one asked for it through the gate. No health colour: a reading is not health.
 */
function InstallationReadingTag({ reading }: { reading: InstallationReading }) {
  return (
    <span
      className="inline-flex max-w-full min-w-0 shrink-0 items-center gap-1 rounded border border-indigo-300/30 bg-indigo-300/5 px-1.5 py-0.5 text-[10px] text-indigo-200"
      title={
        reading.agent
          ? `Read with ${reading.reader}, when an agent asked through the agent gate. The name is the one its client declared; DevHQ cannot verify it.`
          : `Read with ${reading.reader}, when you asked.`
      }
    >
      <ScanSearch className="size-2.5 shrink-0" aria-hidden="true" />
      <span className="truncate">
        Automated · Local read{reading.agent ? ` · asked by ${reading.agent}` : ''}
      </span>
    </span>
  )
}

/** When and where a reading was made, and who asked: always shown with what it found. */
function Headline({ reading, now }: { reading: InstallationReading; now: number }) {
  return (
    <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
      <span
        className={cn(
          'rounded border px-1.5 py-0.5 text-[10px]',
          reading.outcome === 'read' ? 'text-foreground' : 'border-dashed text-amber-200/90',
        )}
      >
        {outcomeLabels[reading.outcome]}
      </span>
      <span>
        <Moment value={reading.observedAt} now={now} /> on <MachineName id={reading.machineId} />
      </span>
      <InstallationReadingTag reading={reading} />
    </p>
  )
}

/** Where the system really kept the file, when that was not the path declared. */
function StoredElsewhere({ reading }: { reading: InstallationReading }) {
  if (!reading.storedAt) return null
  return (
    <p className="text-[11px] break-words text-amber-200/90">
      The system kept this file at <span className="font-mono">{reading.storedAt}</span>, not at the
      path declared: a link, a junction or, for a DevHQ started from a packaged app, that app&apos;s
      own copy of AppData leads there. A DevHQ started another way may find another file, or none.
    </p>
  )
}

/**
 * What a reading found, in the past: what the file was when it was read, which may no longer be
 * so. Only what the reading keeps: nothing else of the file, and of the processes, two numbers.
 */
function Facts({ reading, now }: { reading: InstallationReading; now: number }) {
  if (reading.outcome !== 'read')
    return (
      <p className="mt-1.5 text-[11px] leading-5 break-words text-muted-foreground">
        {reading.detail}
      </p>
    )
  return (
    <dl
      aria-label="What the reading found"
      className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 rounded-md border border-indigo-300/10 bg-indigo-300/[0.03] px-3 py-2 text-[11px] text-muted-foreground"
    >
      <dt>Version</dt>
      <dd className="break-words">
        {reading.versionUnknown !== null ? (
          `Unknown: ${reading.versionUnknown}`
        ) : (
          <>
            <span className="text-foreground/85">
              {reading.productVersion ?? 'No ProductVersion declared'}
            </span>
            {reading.fileVersion && ` · file version ${reading.fileVersion}`}
            {reading.productName && ` · ${reading.productName}`}
          </>
        )}
      </dd>
      <dt>Build</dt>
      <dd className="break-all">
        SHA-256 <span className="font-mono text-foreground/85">{reading.sha256}</span>
      </dd>
      <dt>File</dt>
      <dd>
        {sizeText(reading.sizeBytes ?? 0)}
        {reading.modifiedAt ? (
          <>
            , modified <Moment value={reading.modifiedAt} now={now} />
          </>
        ) : (
          ', modification time unknown'
        )}
      </dd>
      <dt>Processes</dt>
      <dd className="break-words">{processesText(reading)}</dd>
    </dl>
  )
}

/**
 * A declared installation under its reference: its newest reading, what it says against what the
 * inventory declares, since when the build is the same, and Read when this computer is the
 * installation's workstation.
 */
export function InstallationReadingView({ installation }: { installation: Installation }) {
  const {
    snapshot,
    now,
    thisWorkstation,
    workstationError,
    readInstallations,
    readingInstallations,
  } = useInventory()
  const [outcome, setOutcome] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const here = installedOnThisComputer(installation, thisWorkstation)
  const running = readingInstallations.includes(installation.referenceId)
  const latest = installation.latest
  const version = snapshot
    ? versionText(installation, declaredVersion(snapshot, installation))
    : null
  const read = async () => {
    setOutcome(null)
    setError(null)
    try {
      const result = await readInstallations([installation.referenceId])
      const failure = result.failures[0]
      if (failure) setError(`${failure.message} Nothing was kept.`)
      else setOutcome('Read and kept. What it found is above.')
    } catch (cause) {
      setError(inventoryError(cause))
    }
  }
  return (
    <div className="mt-2 space-y-1.5">
      {latest ? (
        <>
          <Headline reading={latest} now={now} />
          <Facts reading={latest} now={now} />
          <StoredElsewhere reading={latest} />
          {installation.movedSinceRead && (
            <p className="text-[11px] break-words text-amber-200/90">
              This reading was made at <span className="font-mono">{latest.path}</span> on{' '}
              <MachineName id={latest.machineId} />; the installation is declared elsewhere now.
            </p>
          )}
          {version && (
            <p
              className={cn(
                'text-[11px]',
                installation.version === 'differs' ? 'text-amber-200/90' : 'text-muted-foreground',
              )}
            >
              {version}
            </p>
          )}
          {installation.build && (
            <p className="text-[11px] text-muted-foreground">{buildText(installation.build)}</p>
          )}
        </>
      ) : (
        <p className="text-[11px] text-muted-foreground">
          <PackageSearch className="mr-1 inline size-3" aria-hidden="true" />
          Never read: DevHQ knows nothing of what is installed here.
        </p>
      )}
      {here ? (
        <div className="flex flex-wrap items-center gap-2 pt-0.5">
          <Button
            size="sm"
            variant="outline"
            disabled={running}
            aria-label={`Read ${installation.path}`}
            onClick={() => void read()}
          >
            <ScanSearch className="size-3.5" aria-hidden="true" />
            {running ? 'Reading…' : 'Read'}
          </Button>
          <p role="status" className="text-[11px] text-muted-foreground empty:hidden">
            {running
              ? 'Reading the file as data, without running it, for up to 30 s. You can keep working.'
              : outcome}
          </p>
        </div>
      ) : thisWorkstation?.resourceId ? (
        <p className="text-[11px] text-muted-foreground">
          DevHQ on <MachineName id={installation.machineId} /> reads it; this computer is{' '}
          <MachineName id={thisWorkstation.resourceId} />.
        </p>
      ) : (
        <p className="text-[11px] text-muted-foreground">
          DevHQ reads only installations on this computer, and does not know which workstation it
          is: {workstationError ? `${workstationError} ` : ''}
          <Link className="text-primary hover:underline" to="/settings">
            choose it in Settings
          </Link>
          .
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs break-words text-rose-300">
          {error}
        </p>
      )}
    </div>
  )
}

/**
 * Every reading of a system's installations, newest first and a page at a time, behind a toggle:
 * closed, it loads nothing, and closing it forgets what was loaded.
 */
export function InstallationHistoryToggle({ subject, name }: { subject: EntityRef; name: string }) {
  const [open, setOpen] = useState(false)
  const region = useId()
  return (
    <div>
      <Button
        size="sm"
        variant="ghost"
        className="-ml-2"
        aria-expanded={open}
        aria-controls={region}
        onClick={() => setOpen(!open)}
      >
        <ChevronDown
          className={cn('size-3.5 transition-transform', open && 'rotate-180')}
          aria-hidden="true"
        />
        {open ? 'Hide installation readings' : 'Show every installation reading'}
      </Button>
      <div
        id={region}
        role="region"
        aria-label={`Installation readings of ${name}`}
        hidden={!open}
        className="mt-3"
      >
        {open && <InstallationHistory subject={subject} name={name} />}
      </div>
    </div>
  )
}

/** The readings a history holds, newest first, and whether older ones are kept beyond them. */
type Loaded = { readings: InstallationReading[]; more: boolean }

/**
 * The newest page put in front of what the history already holds, as the history of copy readings
 * does: when the page reaches a reading already held, both together are every reading down to the
 * oldest held; when it reaches none, the history starts again from the page, so no gap is hidden.
 */
function withNewest(held: Loaded | null, page: Loaded): Loaded {
  const fresh = new Set(page.readings.map((item) => item.id))
  if (!held || !held.readings.some((item) => fresh.has(item.id))) return page
  return {
    readings: [...page.readings, ...held.readings.filter((item) => !fresh.has(item.id))],
    more: held.more,
  }
}

function InstallationHistory({ subject, name }: { subject: EntityRef; name: string }) {
  const { now, olderInstallationReadings, snapshot } = useInventory()
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const { type, id } = subject
  const own = (snapshot?.installations ?? []).filter(
    (item) => item.subject.type === type && item.subject.id === id,
  )
  const declared = new Set(own.map((item) => item.referenceId))
  // The first page when opened, and again whenever the inventory brings a newer reading of one of
  // the system's installations (Read here, or an agent through the gate). Keyed by those
  // readings' ids, not by the objects naming them, which each render makes anew.
  const newest = own.map((item) => item.latest?.id ?? '').join(' ')
  useEffect(() => {
    let current = true
    olderInstallationReadings({ type, id } as EntityRef, null).then(
      (page) => {
        if (!current) return
        setLoaded((held) => withNewest(held, page))
        setError(null)
      },
      (cause) => current && setError(inventoryError(cause)),
    )
    return () => {
      current = false
    }
  }, [olderInstallationReadings, type, id, newest])
  const loadOlder = async (before: string) => {
    setLoading(true)
    setError(null)
    try {
      const page = await olderInstallationReadings(subject, before)
      setLoaded((held) => {
        const seen = new Set((held?.readings ?? []).map((item) => item.id))
        return {
          readings: [
            ...(held?.readings ?? []),
            ...page.readings.filter((item) => !seen.has(item.id)),
          ],
          more: page.more,
        }
      })
    } catch (cause) {
      setError(inventoryError(cause))
    } finally {
      setLoading(false)
    }
  }
  const readings = loaded?.readings ?? null
  return (
    <div className="space-y-2 text-[11px]">
      {error && (
        <p role="alert" className="break-words text-rose-300">
          {error}
        </p>
      )}
      {readings && !readings.length && (
        <p className="text-muted-foreground">No installation of {name} was ever read.</p>
      )}
      {readings && readings.length > 0 && (
        <ul className="divide-y border-y">
          {readings.map((reading) => (
            <li key={reading.id} className="space-y-1 py-2">
              <Headline reading={reading} now={now} />
              <p className="font-mono break-all text-muted-foreground">
                {reading.path}
                {!declared.has(reading.referenceId) && (
                  <span className="font-sans"> · no longer declared by {name}</span>
                )}
              </p>
              <p className="break-words text-foreground/85">{installationSummary(reading)}</p>
            </li>
          ))}
        </ul>
      )}
      <p role="status" className="text-muted-foreground">
        {!readings && !error
          ? 'Loading readings…'
          : readings && readings.length > 0
            ? loaded?.more
              ? `The newest ${counted(readings.length, 'reading')}; older ones are kept.`
              : `All ${counted(readings.length, 'reading')} of its installations.`
            : ''}
      </p>
      {loaded?.more && readings && readings.length > 0 && (
        <Button
          size="sm"
          variant="outline"
          disabled={loading}
          onClick={() => void loadOlder(readings.at(-1)!.id)}
        >
          {loading ? 'Loading…' : `Load ${readingsPage} older readings`}
        </Button>
      )}
    </div>
  )
}
