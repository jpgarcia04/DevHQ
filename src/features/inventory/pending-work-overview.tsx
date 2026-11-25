import { useState } from 'react'
import { subjectsOf } from '@/domain/evidence'
import { counted } from '@/shared/count'
import { relativeTime } from '@/shared/time'
import { MachineName } from './copy-readings'
import { SubjectLink } from './evidence'
import { declaredVersion } from './installations'
import { Button } from '@/components/ui/button'
import type {
  GithubRead,
  GithubRepository,
  Installation,
  InstallationsRead,
  InventorySnapshot,
  LocalCopy,
  ReadOutcome,
} from './inventory-api'
import { useInventory } from './inventory-context'
import type { OpeningGithub, OpeningRead } from './inventory-context'
import { unpublishedText } from './local-copies'
import {
  failedConclusions,
  oldReadingMs,
  pendingWorkOf,
  waitingMs,
  type PendingItem,
  type PendingReason,
} from './pending'

const day = 24 * 60 * 60 * 1000
const oldReadingDays = oldReadingMs / day
const waitingDays = waitingMs / day

/** Names in a short list: «CI», «CI and Lint», «CI, Lint and Deploy». */
function listed(names: string[]) {
  return names.length < 2
    ? (names[0] ?? '')
    : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
}

/** What a repository's reading says for one reason. */
function repositoryText(reason: PendingReason, repository: GithubRepository | undefined) {
  const reading = repository?.latest
  switch (reason) {
    case 'main_failing': {
      const failed = (reading?.workflows ?? []).filter((run) =>
        failedConclusions.includes(run.conclusion),
      )
      return `${listed(failed.map((run) => run.workflow))} did not pass on ${reading?.defaultBranch ?? 'the default branch'}`
    }
    case 'pr_waiting': {
      const readAt = reading ? Date.parse(reading.observedAt) : 0
      const waiting = (reading?.pullRequests ?? []).filter(
        (pull) => readAt - Date.parse(pull.updatedAt) > waitingMs,
      )
      const drafts = waiting.filter((pull) => pull.draft).length
      return `${counted(waiting.length, 'pull request')} unchanged for more than ${counted(waitingDays, 'day')} (${listed(waiting.map((pull) => `#${pull.number}`))})${drafts ? `, ${counted(drafts, 'draft')} among them` : ''}`
    }
    case 'not_found':
      return `not read: ${reading?.detail?.replace(/\.$/, '') ?? 'GitHub did not answer'}`
    case 'never_read':
      return 'never read on GitHub'
    case 'old_reading':
      return `read more than ${counted(oldReadingDays, 'day')} ago`
    default:
      return reason
  }
}

/** One reason in a few words, from the reading it rests on. */
function reasonText(
  reason: PendingReason,
  snapshot: InventorySnapshot,
  copy: LocalCopy | undefined,
  installation: Installation | undefined,
) {
  const reading = copy?.latest
  switch (reason) {
    case 'operation':
      return `a ${reading?.operation?.replace('_', '-') ?? 'git operation'} left halfway`
    case 'changes':
      return [
        reading?.changed ? counted(reading.changed, 'changed file') : '',
        reading?.untracked ? counted(reading.untracked, 'new entry', 'new entries') : '',
        reading?.unverifiable ? counted(reading.unverifiable, 'unverifiable file') : '',
      ]
        .filter(Boolean)
        .join(', ')
    case 'unpublished': {
      const commits = (reading?.unpublishedOnHead ?? 0) + (reading?.unpublishedElsewhere ?? 0)
      return `${unpublishedText(commits)} ${commits === 1 ? 'commit' : 'commits'} on no remote`
    }
    case 'stashes':
      return counted(reading?.stashes ?? 0, 'stash', 'stashes')
    case 'version_differs':
      return `the file declared ${installation?.latest?.productVersion ?? 'a version'}, the inventory ${
        installation ? (declaredVersion(snapshot, installation) ?? 'none') : 'another'
      }`
    case 'not_installed':
      return installation?.againstCopy?.state === 'newer_commits'
        ? 'the local copy held a commit newer than the installed file'
        : `the installed build is commit ${installation?.againstCopy?.installedCommit?.slice(0, 12) ?? 'unknown'}, the local copy's HEAD was ${installation?.againstCopy?.copyHead?.slice(0, 12) ?? 'unknown'}`
    case 'main_failing':
    case 'pr_waiting':
      return reason
    case 'not_found':
      return 'not found where declared, or not readable'
    case 'never_read':
      return 'never read on this computer'
    case 'old_reading':
      return `read more than ${counted(oldReadingDays, 'day')} ago`
  }
}

function RepositoryRow({ item }: { item: PendingItem }) {
  const { snapshot, now } = useInventory()
  const repository = snapshot?.githubRepositories.find(
    (each) => each.referenceId === item.referenceId,
  )
  const reading = repository?.latest
  const reasons = item.reasons.map((reason) => repositoryText(reason, repository))
  return (
    <li className="space-y-0.5 px-4 py-3">
      <p className="flex flex-wrap items-center gap-x-2">
        <SubjectLink subject={item.subject} className="min-w-0 truncate text-foreground" />
        <span className="text-[11px] text-muted-foreground">Repository on GitHub</span>
      </p>
      {repository && (
        <p className="font-mono text-[11px] break-all text-muted-foreground">
          {repository.repository}
        </p>
      )}
      <p className="text-[11px] text-foreground/85">
        {reasons.join(' · ').replace(/^./, (first) => first.toUpperCase())}
      </p>
      {reading && (
        <p className="text-[11px] text-muted-foreground">
          Read on GitHub {relativeTime(reading.observedAt, now).toLowerCase()}
          {reading.agent ? `, asked by ${reading.agent}` : ''}
          {repository?.movedSinceRead ? `, when the reference named ${reading.repository}` : ''}
        </p>
      )}
    </li>
  )
}

function PendingRow({ item }: { item: PendingItem }) {
  const { snapshot, now } = useInventory()
  if (!snapshot) return null
  if (item.kind === 'repository') return <RepositoryRow item={item} />
  const copy =
    item.kind === 'localCopy'
      ? snapshot.localCopies.find((each) => each.referenceId === item.referenceId)
      : undefined
  const installation =
    item.kind === 'installation'
      ? snapshot.installations.find((each) => each.referenceId === item.referenceId)
      : undefined
  const declared = copy ?? installation
  const reading = copy?.latest ?? installation?.latest
  const reasons = item.reasons.map((reason) => reasonText(reason, snapshot, copy, installation))
  return (
    <li className="space-y-0.5 px-4 py-3">
      <p className="flex flex-wrap items-center gap-x-2">
        <SubjectLink subject={item.subject} className="min-w-0 truncate text-foreground" />
        <span className="text-[11px] text-muted-foreground">
          {item.kind === 'localCopy' ? 'Local copy' : 'Installation'}
          {declared && (
            <>
              {' '}
              on <MachineName id={declared.machineId} />
            </>
          )}
        </span>
      </p>
      {declared && (
        <p className="font-mono text-[11px] break-all text-muted-foreground">{declared.path}</p>
      )}
      <p className="text-[11px] text-foreground/85">
        {reasons.join(' · ').replace(/^./, (first) => first.toUpperCase())}
      </p>
      {reading && (
        <p className="text-[11px] text-muted-foreground">
          Read {relativeTime(reading.observedAt, now).toLowerCase()}
          {reading.agent ? `, asked by ${reading.agent}` : ''}
        </p>
      )}
    </li>
  )
}

/** What one side of the opening read did, in a few words. */
function sideText(
  outcome: ReadOutcome | InstallationsRead | GithubRead | string | null,
  one: string,
  many: string,
) {
  if (outcome === null) return null
  if (typeof outcome === 'string')
    return `failed to read the ${many} (${outcome.replace(/\.$/, '')})`
  const failed = outcome.failures.length
  return `read ${counted(outcome.readings.length, one, many)}${failed ? ` (${failed} could not be read and kept nothing)` : ''}`
}

function openingText(read: OpeningRead | null) {
  if (!read) return null
  if (read.state === 'reading')
    return "Reading this computer's local copies and installations, as DevHQ does when it opens. You can keep working."
  const sides = [
    sideText(read.copies, 'local copy', 'local copies'),
    sideText(read.installations, 'installation', 'installations'),
  ].filter(Boolean)
  return sides.length ? `When DevHQ opened, it ${sides.join('; ')}.` : null
}

function githubText(read: OpeningGithub | null) {
  if (!read) return null
  if (read.state === 'no_token')
    return 'No GitHub token is kept, so DevHQ did not read the repositories on GitHub when it opened: save one in Settings.'
  if (read.state === 'reading')
    return 'Reading the repositories on GitHub, as DevHQ does when it opens. You can keep working.'
  const side = sideText(read.outcome, 'repository on GitHub', 'repositories on GitHub')
  return side ? `When DevHQ opened, it ${side}.` : null
}

/**
 * Beside «what needs my attention?» and never in it: what the readings of local copies and
 * installations say is waiting, most urgent first (see `pending.ts`). The agent gate gives the same
 * list as `pendingWork` in `attention`.
 */
export function PendingWorkOverview() {
  const { snapshot, thisWorkstation, now, openingRead, openingGithub, readGithub, readingGithub } =
    useInventory()
  const [githubError, setGithubError] = useState<string | null>(null)
  if (!snapshot || !subjectsOf(snapshot).length) return null
  const items = pendingWorkOf(snapshot, thisWorkstation, now)
  const declared =
    snapshot.localCopies.length + snapshot.installations.length + snapshot.githubRepositories.length
  const opening = openingText(openingRead)
  const github = githubText(openingGithub)
  const readOnGithub = () => {
    setGithubError(null)
    readGithub(null).catch((cause: unknown) =>
      setGithubError(cause instanceof Error ? cause.message : String(cause)),
    )
  }
  return (
    <section aria-labelledby="pending-work-heading">
      <h2 id="pending-work-heading" className="section-heading mb-3">
        Pending work{' '}
        <span className="ml-2 rounded bg-sky-300/10 px-1.5 py-0.5 font-mono text-[10px] text-sky-200">
          {items.length}
        </span>
      </h2>
      <div className="panel overflow-hidden text-xs">
        {opening && (
          <p
            role="status"
            className="border-b px-4 py-2.5 text-[11px] break-words text-muted-foreground"
          >
            {opening}
          </p>
        )}
        {github && (
          <p
            role="status"
            className="border-b px-4 py-2.5 text-[11px] break-words text-muted-foreground"
          >
            {github}
          </p>
        )}
        {items.length ? (
          <ul className="divide-y">
            {items.map((item) => (
              <PendingRow key={`${item.kind}:${item.referenceId}`} item={item} />
            ))}
          </ul>
        ) : (
          <p className="px-5 py-6 text-center text-sm text-muted-foreground">
            {declared
              ? 'No reading says work is waiting in a local copy, an installation or a repository on GitHub.'
              : 'No local copy, installation or repository on GitHub is declared. DevHQ reads only what the inventory names: add them as references of an application or resource.'}
          </p>
        )}
        {snapshot.githubRepositories.length > 0 && (
          <div className="flex flex-wrap items-center gap-3 border-t px-4 py-2.5">
            <Button size="sm" variant="outline" disabled={readingGithub} onClick={readOnGithub}>
              {readingGithub ? 'Reading GitHub…' : 'Read GitHub'}
            </Button>
            {githubError && (
              <p role="alert" className="text-[11px] text-rose-300">
                {githubError}
              </p>
            )}
          </div>
        )}
        <p className="border-t px-4 py-2.5 text-[11px] leading-5 text-muted-foreground">
          A default branch whose checks did not pass first, then work left halfway, uncommitted or
          on no remote, then pull requests unchanged for more than {counted(waitingDays, 'day')},
          installations that declared another version or are not what the local copy held, and what
          was not found, never read or read more than {counted(oldReadingDays, 'day')} ago. What
          each copy, file or repository held when it was read, which may no longer be so. Not
          health: none of it is in Needs attention.
        </p>
      </div>
    </section>
  )
}
