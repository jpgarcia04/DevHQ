import { counted } from '@/shared/count'
import { formatBytes } from '@/shared/size'
import { formatDateTime } from '@/shared/time'
import type {
  Installation,
  InstallationAgainstCopy,
  InstallationBuild,
  InstallationReading,
  InventorySnapshot,
  ThisWorkstation,
} from './inventory-api'
import { archived, thisMachine } from './local-copies'

/** Whether an installation is on this computer, so that this DevHQ can read it. */
export function installedOnThisComputer(
  installation: Installation,
  workstation: ThisWorkstation | null,
) {
  const machine = thisMachine(workstation)
  return machine !== null && installation.machineId === machine
}

/**
 * The installations Read all reads, as the native process chooses them: every one of this
 * workstation but those of archived applications. Empty when DevHQ does not know which workstation
 * this is.
 */
export function readAllInstallations(
  snapshot: InventorySnapshot,
  workstation: ThisWorkstation | null,
) {
  return snapshot.installations.filter(
    (installation) =>
      installedOnThisComputer(installation, workstation) &&
      !archived(snapshot, installation.subject),
  )
}

/**
 * What Overview says of the installations, beside «what needs my attention?» and never in it: they
 * are not health. Installations of archived applications are left out. The same lists as the agent
 * gate's `installations` in `attention`.
 */
export function installationsOverview(
  snapshot: InventorySnapshot,
  workstation: ThisWorkstation | null,
) {
  const current = snapshot.installations.filter(
    (installation) => !archived(snapshot, installation.subject),
  )
  const machine = thisMachine(workstation)
  return {
    current,
    /** The file declared another version than its application declares in the inventory. */
    versionDiffers: current.filter((installation) => installation.version === 'differs'),
    /** Not where declared, not readable, or no executable, when last read. */
    notFound: current.filter(
      (installation) => installation.latest && installation.latest.outcome !== 'read',
    ),
    neverReadHere: current.filter(
      (installation) => !installation.latest && installation.machineId === machine,
    ),
    /** Never read on other computers, or anywhere while DevHQ does not know this one. */
    neverReadElsewhere: current.filter(
      (installation) => !installation.latest && installation.machineId !== machine,
    ),
  }
}

/** The version a system declares in the inventory: an application's; a resource declares none. */
export function declaredVersion(snapshot: InventorySnapshot, installation: Installation) {
  return installation.subject.type === 'application'
    ? (snapshot.applications.find((app) => app.id === installation.subject.id)?.version ?? null)
    : null
}

/**
 * How the declared version compared with the one the file declared, in the past and with both
 * said: which of the two is out of date, DevHQ cannot know. Null when there is nothing to compare.
 */
export function versionText(installation: Installation, declared: string | null) {
  const installed = installation.latest?.productVersion
  switch (installation.version) {
    case 'matches':
      return `The inventory declares version ${declared}, the one the file declared.`
    case 'differs':
      return `The inventory declares version ${declared}; the file declared ${installed}. One of the two is out of date.`
    case 'undeclared':
      return `The file declared ${installed}; the inventory declares no version to compare it with.`
    case 'unknown':
      return declared
        ? `The file's version is unknown, so DevHQ cannot say whether it was the declared ${declared}.`
        : "The file's version is unknown."
    default:
      return null
  }
}

/**
 * How the installed build compared with what a local copy held when it was read, in the past and
 * saying what DevHQ cannot know: with a declared commit, whether it was HEAD's, never whether the
 * copy was ahead or behind; without one, only whether a newer commit existed, never whether the
 * build holds it.
 */
export function againstCopyText(against: InstallationAgainstCopy) {
  const short = (commit: string | null) => commit?.slice(0, 12) ?? 'unknown'
  switch (against.state) {
    case 'head':
      return `The file declared commit ${short(against.installedCommit)}, the one HEAD was on in the local copy's newest reading.`
    case 'other_commit':
      return `The file declared commit ${short(against.installedCommit)}; HEAD was on ${short(against.copyHead)} in the local copy's newest reading. What is installed is not what the copy held: DevHQ does not say whether it is older or newer.`
    case 'newer_commits':
      return "The file declared no commit, and the local copy's newest reading held a commit dated after the file was last modified. It may or may not be in the installed build."
    case 'not_newer':
      return "The file declared no commit, and no commit in the local copy's newest reading was dated after the file was last modified."
    case 'unknown':
      return "DevHQ could not compare the installed build with the local copy's newest reading."
  }
}

/**
 * Since when the file is the same build, as far as the readings say, with both moments when it
 * changed: DevHQ knows a build only when it read the file, never when it was installed.
 */
export function buildText(build: InstallationBuild) {
  const since = formatDateTime(build.sameSince)
  if (build.previous)
    return `Another build was there on ${formatDateTime(build.previous.observedAt)}${build.previous.productVersion ? ` (it declared ${build.previous.productVersion})` : ''}, and this one on ${since}: it changed between those two readings. DevHQ does not know when.`
  if (!build.complete)
    return `The same build in the last ${counted(build.readings, 'reading')}, since at least ${since}. DevHQ looked no further back.`
  if (build.readings === 1)
    return 'Read once: there is no earlier reading to compare its build with.'
  return `The same build in all ${counted(build.readings, 'reading')}, since ${since}.`
}

/**
 * The processes that were running from the file, as two numbers and in the past: never that the
 * application is open, which a count of processes does not say.
 */
export function processesText(reading: InstallationReading) {
  if (reading.processesUnknown !== null) return `Unknown: ${reading.processesUnknown}`
  const running = reading.processes ?? 0
  const unexamined = reading.processesUnexamined ?? 0
  const found = running
    ? `${counted(running, 'process', 'processes')} ${running === 1 ? 'was' : 'were'} running from it`
    : 'None was running from it'
  return unexamined
    ? `${found}; the system did not let DevHQ look at ${counted(unexamined, 'other process', 'other processes')} named like it`
    : found
}

/** A file's size as people read it, with the exact count: «22.5 MB (23,620,096 bytes)». */
export function sizeText(bytes: number) {
  const exact = counted(bytes, 'byte')
  return bytes < 1024 ? exact : `${formatBytes(bytes)} (${exact})`
}

/** What a reading found, in one line, for a list of many: the history of a system. */
export function installationSummary(reading: InstallationReading) {
  if (reading.outcome !== 'read') return reading.detail ?? ''
  const version = reading.productVersion
    ? `Declared ${reading.productVersion}`
    : `Version unknown: ${reading.versionUnknown ?? ''}`
  return [version, `build ${reading.sha256?.slice(0, 12)}`, processesText(reading)].join(' · ')
}
