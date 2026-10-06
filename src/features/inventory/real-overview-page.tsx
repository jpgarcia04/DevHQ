import { Boxes, ChevronRight, Layers3, Server } from 'lucide-react'
import { Link } from 'react-router-dom'
import { PageHeader } from '@/components/page-header'
import {
  attentionItems,
  evidenceCoverage,
  subjectEvidence,
  subjectKey,
  subjectsOf,
  workspaceStatus,
  type Evidence,
  type EvidenceState,
} from '@/domain/evidence'
import { relativeTime } from '@/shared/time'
import { cn } from '@/shared/utils'
import { EvidenceBadge, SourceTag, SubjectLink } from './evidence'
import { automatedSince, timelineEvents } from './history'
import { useInventory } from './inventory-context'
import { InventoryState } from './inventory-state'
import { AttentionList, EventRecordList } from './record-lists'
import { SupervisionOverview } from './supervision-overview'

// Problems first, then missing or expired evidence, then current healthy results.
const coverageOrder: EvidenceState[] = ['unhealthy', 'degraded', 'stale', 'unobserved', 'healthy']

function coverageNote(evidence: Evidence, now: number) {
  if (!evidence.latest) return 'Never observed'
  if (evidence.state === 'stale')
    return `Expired ${relativeTime(evidence.latest.validUntil, now).toLowerCase()}`
  return `Observed ${relativeTime(evidence.latest.observedAt, now).toLowerCase()}`
}

export function RealOverviewPage() {
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Local inventory · recorded evidence"
        title="Overview"
        description="What needs your attention, judged only from evidence recorded in DevHQ."
      />
      <InventoryState>
        <OverviewContent />
      </InventoryState>
    </div>
  )
}

function OverviewContent() {
  const { snapshot, now } = useInventory()
  if (!snapshot) return null
  const subjects = subjectsOf(snapshot)
  const attention = attentionItems(subjects, snapshot.observations, snapshot.incidents, now)
  const coverage = evidenceCoverage(subjects, snapshot.observations, now)
  const status = workspaceStatus(attention, coverage)
  const evidence = subjects
    .map((subject) => ({ subject, evidence: subjectEvidence(snapshot.observations, subject, now) }))
    .sort(
      (a, b) => coverageOrder.indexOf(a.evidence.state) - coverageOrder.indexOf(b.evidence.state),
    )
  const summary = {
    empty: [
      'Nothing is registered yet.',
      'Add applications and resources, then record what you observe.',
    ],
    attention: [
      `${attention.length} ${attention.length === 1 ? 'item needs' : 'items need'} your attention.`,
      'Open incidents, observed problems and expired evidence are listed below.',
    ],
    unobserved: [
      'No recorded problems, but coverage is incomplete.',
      `${coverage.unobserved} of ${subjects.length} ${subjects.length === 1 ? 'system has' : 'systems have'} never been observed. DevHQ does not assume ${coverage.unobserved === 1 ? 'it is' : 'they are'} healthy.`,
    ],
    covered: [
      'Every system has current healthy evidence.',
      'Based on manual entries, agent entries and HTTP checks that are still within their validity.',
    ],
  }[status]
  return (
    <>
      <section
        aria-label="Ecosystem status"
        className={cn(
          'panel flex flex-wrap items-center justify-between gap-6 border-l-2 px-6 py-5',
          status === 'attention'
            ? 'border-l-amber-300/70'
            : status === 'covered'
              ? 'border-l-emerald-300/60'
              : 'border-l-muted-foreground/50',
        )}
      >
        <div className="min-w-0">
          <p className="eyebrow mb-2.5">Ecosystem status</p>
          <h2 className="text-lg font-medium tracking-tight">{summary[0]}</h2>
          <p className="mt-1.5 text-xs text-muted-foreground">{summary[1]}</p>
        </div>
        <dl className="flex gap-6 text-xs">
          {(
            [
              ['Current evidence', coverage.current],
              ['Expired', coverage.stale],
              ['Never observed', coverage.unobserved],
            ] as const
          ).map(([label, count]) => (
            <div key={label}>
              <dt className="text-[11px] text-muted-foreground">{label}</dt>
              <dd className="mt-1 text-lg font-semibold">{count}</dd>
            </div>
          ))}
        </dl>
      </section>
      <div className="grid gap-7 min-[1180px]:grid-cols-[minmax(0,1.6fr)_minmax(280px,1fr)]">
        <div className="space-y-7">
          <section aria-labelledby="attention-heading">
            <div className="mb-3 flex items-center justify-between">
              <h2 id="attention-heading" className="section-heading">
                Needs attention{' '}
                <span className="ml-2 rounded bg-amber-300/10 px-1.5 py-0.5 font-mono text-[10px] text-amber-200">
                  {attention.length}
                </span>
              </h2>
              <Link className="quiet-link" to="/incidents">
                All incidents <ChevronRight className="size-3" />
              </Link>
            </div>
            <div className="panel overflow-hidden">
              {attention.length ? (
                <AttentionList items={attention} />
              ) : (
                <p className="px-5 py-8 text-center text-sm text-muted-foreground">
                  No open incidents, observed problems or expired evidence.
                  {coverage.unobserved > 0 &&
                    ' Systems that were never observed are not included, so this is not a clean bill of health.'}
                </p>
              )}
            </div>
          </section>
          <section aria-labelledby="coverage-heading">
            <h2 id="coverage-heading" className="section-heading mb-3">
              Evidence coverage
            </h2>
            {evidence.length ? (
              <ul className="panel divide-y">
                {evidence.map((item) => (
                  <li
                    key={subjectKey(item.subject)}
                    className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3"
                  >
                    {item.subject.type === 'application' ? (
                      <Layers3 className="size-4 text-muted-foreground" aria-hidden="true" />
                    ) : (
                      <Server className="size-4 text-muted-foreground" aria-hidden="true" />
                    )}
                    <SubjectLink
                      subject={item.subject}
                      className="min-w-0 flex-1 truncate text-[13px] text-foreground"
                    />
                    <span className="text-[11px] text-muted-foreground">
                      {coverageNote(item.evidence, now)}
                    </span>
                    {item.evidence.latest && <SourceTag record={item.evidence.latest} />}
                    <EvidenceBadge state={item.evidence.state} compact />
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-xs text-muted-foreground">
                No applications or resources are registered.
              </p>
            )}
          </section>
        </div>
        <div className="space-y-7">
          <SupervisionOverview />
          <section aria-labelledby="activity-heading">
            <div className="mb-3 flex items-center justify-between">
              <h2 id="activity-heading" className="section-heading">
                Recent activity
              </h2>
              <Link className="quiet-link" to="/activity">
                Timeline <ChevronRight className="size-3" />
              </Link>
            </div>
            <div className="panel p-5">
              {snapshot.events.length || snapshot.changes.length ? (
                <EventRecordList
                  items={timelineEvents(snapshot)}
                  foldRepeats
                  maxRows={5}
                  detailed={false}
                  automatedSince={automatedSince(snapshot.automatedHistory, subjects)}
                />
              ) : (
                <p className="text-xs text-muted-foreground">No activity recorded yet.</p>
              )}
            </div>
          </section>
          <div className="grid grid-cols-2 gap-4">
            <Link
              to="/applications"
              className="panel flex items-center gap-3 p-4 hover:border-muted-foreground/40"
            >
              <Layers3 className="size-4 text-primary" aria-hidden="true" />
              <span>
                <span className="block text-xl font-semibold">{snapshot.applications.length}</span>
                <span className="text-xs text-muted-foreground">Applications</span>
              </span>
            </Link>
            <Link
              to="/infrastructure"
              className="panel flex items-center gap-3 p-4 hover:border-muted-foreground/40"
            >
              <Boxes className="size-4 text-primary" aria-hidden="true" />
              <span>
                <span className="block text-xl font-semibold">{snapshot.resources.length}</span>
                <span className="text-xs text-muted-foreground">Resources</span>
              </span>
            </Link>
          </div>
        </div>
      </div>
    </>
  )
}
