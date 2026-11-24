import { useState, type ReactNode } from 'react'
import { ScanSearch } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { subjectsOf } from '@/domain/evidence'
import { counted } from '@/shared/count'
import { relativeTime } from '@/shared/time'
import { MachineName } from './copy-readings'
import { SubjectLink } from './evidence'
import { declaredVersion, installationsOverview, readAllInstallations } from './installations'
import { inventoryError, type Installation, type InstallationsRead } from './inventory-api'
import { useInventory } from './inventory-context'

const notFoundText = {
  missing: 'nothing was at the path',
  unreadable: 'not readable',
  not_an_executable: 'not an executable',
  read: '',
}

function InstallationRow({
  installation,
  children,
}: {
  installation: Installation
  children?: ReactNode
}) {
  const { now } = useInventory()
  const latest = installation.latest
  return (
    <li className="space-y-0.5 py-2.5">
      <p className="flex flex-wrap items-center gap-x-2">
        <SubjectLink subject={installation.subject} className="min-w-0 truncate text-foreground" />
        <span className="text-[11px] text-muted-foreground">
          on <MachineName id={installation.machineId} />
        </span>
      </p>
      <p className="font-mono text-[11px] break-all text-muted-foreground">{installation.path}</p>
      {children && <p className="text-[11px] text-foreground/85">{children}</p>}
      {installation.movedSinceRead && latest && (
        <p className="text-[11px] break-all text-amber-200/90">
          Read at {latest.path}
          {latest.machineId !== installation.machineId ? ' on another machine' : ''}, before it was
          declared here.
        </p>
      )}
      {latest && (
        <p className="text-[11px] text-muted-foreground">
          Read {relativeTime(latest.observedAt, now).toLowerCase()}
          {latest.agent ? `, asked by ${latest.agent}` : ''}
        </p>
      )}
    </li>
  )
}

/**
 * Beside «what needs my attention?» and never in it: what the declared installations were when
 * they were read, which may no longer be so. The ones whose file declared another version than the
 * inventory, the ones not found where declared, and those of this computer never read, so that an
 * empty list does not reassure more than it should. Installations of archived applications are
 * left out, as from Read all.
 */
export function InstallationsOverview() {
  const { snapshot, thisWorkstation, workstationError, readInstallations, readingInstallations } =
    useInventory()
  const [outcome, setOutcome] = useState<InstallationsRead | null>(null)
  const [error, setError] = useState<string | null>(null)
  if (!snapshot || !subjectsOf(snapshot).length) return null
  const { current, versionDiffers, notFound, neverReadHere, neverReadElsewhere } =
    installationsOverview(snapshot, thisWorkstation)
  const targets = readAllInstallations(snapshot, thisWorkstation)
  const running = targets.some((item) => readingInstallations.includes(item.referenceId))
  const readAll = async () => {
    setOutcome(null)
    setError(null)
    try {
      setOutcome(await readInstallations(null))
    } catch (cause) {
      setError(inventoryError(cause))
    }
  }
  return (
    <section aria-labelledby="installations-heading">
      <h2 id="installations-heading" className="section-heading mb-3">
        Installations
      </h2>
      <div className="panel space-y-4 p-5 text-xs">
        {!current.length ? (
          <p className="leading-5 text-muted-foreground">
            No system declares an installation. DevHQ reads only the installed executables the
            inventory names: add one as an Installation reference of an application or resource.
          </p>
        ) : (
          <>
            <div>
              <p className="font-medium">
                {thisWorkstation?.resourceId ? (
                  <>
                    {counted(targets.length, 'installation')} on this computer,{' '}
                    <MachineName id={thisWorkstation.resourceId} />
                  </>
                ) : (
                  'DevHQ does not know which workstation this computer is'
                )}
              </p>
              <p className="mt-1.5 leading-5 text-muted-foreground">
                {thisWorkstation?.resourceId ? (
                  'DevHQ reads them once when it opens and whenever you ask, one after another, as data: it never runs them. Installations on other computers are read by DevHQ there.'
                ) : (
                  <>
                    {thisWorkstation?.message ?? workstationError ?? 'Asking…'} It reads no
                    installation until{' '}
                    <Link className="text-primary hover:underline" to="/settings">
                      you choose it in Settings
                    </Link>
                    .
                  </>
                )}
              </p>
            </div>
            {targets.length > 0 && (
              <div className="flex flex-wrap items-center gap-3">
                <Button
                  size="sm"
                  variant="outline"
                  disabled={running}
                  aria-label="Read all installations on this computer"
                  onClick={() => void readAll()}
                >
                  <ScanSearch className="size-3.5" aria-hidden="true" />
                  {running ? 'Reading…' : 'Read all on this computer'}
                </Button>
                <p role="status" className="text-[11px] text-muted-foreground empty:hidden">
                  {running
                    ? `Reading ${counted(targets.length, 'installation')}, up to 30 s each. You can keep working.`
                    : outcome &&
                      `Read ${counted(outcome.readings.length, 'installation')}${outcome.failures.length ? `; ${outcome.failures.length} could not be read and kept nothing` : ''}.`}
                </p>
              </div>
            )}
            {outcome && outcome.failures.length > 0 && (
              <ul role="alert" className="space-y-1 text-[11px] break-words text-rose-300">
                {outcome.failures.map((failure) => (
                  <li key={failure.referenceId}>
                    <SubjectLink subject={failure.subject} />: {failure.message}
                  </li>
                ))}
              </ul>
            )}
            {error && (
              <p role="alert" className="break-words text-rose-300">
                {error}
              </p>
            )}
            <Group title="Declared version is not the installed one" installations={versionDiffers}>
              {(installation) =>
                `The inventory declares ${declaredVersion(snapshot, installation)}; the file declared ${installation.latest?.productVersion}.`
              }
            </Group>
            <Group title="Not found where declared" installations={notFound}>
              {(installation) => `When read: ${notFoundText[installation.latest!.outcome]}.`}
            </Group>
            <Group title="Never read on this computer" installations={neverReadHere} />
            {neverReadElsewhere.length > 0 && (
              <p className="leading-5 text-muted-foreground">
                {thisWorkstation?.resourceId
                  ? `${counted(neverReadElsewhere.length, 'installation')} on other computers ${neverReadElsewhere.length === 1 ? 'was' : 'were'} never read; DevHQ on each computer reads its own.`
                  : `${counted(neverReadElsewhere.length, 'installation')} ${neverReadElsewhere.length === 1 ? 'was' : 'were'} never read.`}
              </p>
            )}
            {!versionDiffers.length && !notFound.length && !neverReadHere.length && (
              <p className="leading-5 text-muted-foreground">
                No reading found a version other than the declared one, or an installation missing.
                {neverReadElsewhere.length > 0 && ' Installations never read are not included.'}
              </p>
            )}
          </>
        )}
        <p className="text-[11px] leading-5 text-muted-foreground">
          What each installed file was when it was read, which may no longer be so. Not health: none
          of it is in Needs attention, and a file read says nothing of whether its application
          works.
        </p>
      </div>
    </section>
  )
}

function Group({
  title,
  installations,
  children,
}: {
  title: string
  installations: Installation[]
  children?: (installation: Installation) => string
}) {
  if (!installations.length) return null
  return (
    <div>
      <h3 className="text-[11px] font-medium text-muted-foreground">
        {title} ({installations.length})
      </h3>
      <ul className="mt-1 divide-y border-y">
        {installations.map((installation) => (
          <InstallationRow key={installation.referenceId} installation={installation}>
            {children?.(installation)}
          </InstallationRow>
        ))}
      </ul>
    </div>
  )
}
