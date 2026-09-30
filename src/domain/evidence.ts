import type { EntityRef, IncidentSeverity, Observation, RecordedIncident } from './models'
import { activeIncidents } from './selectors'

/**
 * What DevHQ can honestly say about a subject: nothing was ever observed, the latest
 * observation has expired, or the latest observation is current and reports a result.
 */
export type Evidence =
  | { state: 'unobserved'; latest: null }
  | { state: 'stale' | Observation['result']; latest: Observation }
export type EvidenceState = Evidence['state']
export type ObservedEvidence = Extract<Evidence, { latest: Observation }>

export type AttentionItem =
  | { kind: 'incident'; subject: EntityRef; incident: RecordedIncident }
  | { kind: 'evidence'; subject: EntityRef; evidence: ObservedEvidence }

export interface EvidenceCoverage {
  current: number
  stale: number
  unobserved: number
}

export const sameSubject = (a: EntityRef, b: EntityRef) => a.type === b.type && a.id === b.id
export const subjectKey = (subject: EntityRef) => `${subject.type}:${subject.id}`

export function subjectFromKey(key: string): EntityRef | null {
  const separator = key.indexOf(':')
  const type = key.slice(0, separator)
  const id = key.slice(separator + 1)
  if (separator < 1 || !id || (type !== 'application' && type !== 'resource')) return null
  return { type, id }
}

/** Every application and resource that can hold records. */
export const subjectsOf = (inventory: {
  applications: { id: string }[]
  resources: { id: string }[]
}): EntityRef[] => [
  ...inventory.applications.map(({ id }) => ({ type: 'application' as const, id })),
  ...inventory.resources.map(({ id }) => ({ type: 'resource' as const, id })),
]

/** Records about the application itself or about a resource its components run on. */
export const concernsApplication = (
  subject: EntityRef,
  applicationId: string,
  resourceIds: string[],
) =>
  subject.type === 'application' ? subject.id === applicationId : resourceIds.includes(subject.id)

const newestFirst = (a: Observation, b: Observation) =>
  Date.parse(b.observedAt) - Date.parse(a.observedAt) ||
  Date.parse(b.recordedAt) - Date.parse(a.recordedAt)

export const subjectObservations = (observations: Observation[], subject: EntityRef) =>
  observations.filter((item) => sameSubject(item.subject, subject)).sort(newestFirst)

/** Current until the instant its validity ends; from then on it is expired evidence. */
export const isCurrent = (observation: Observation, now: number) =>
  Date.parse(observation.validUntil) > now

/** The newest observation supersedes older ones, even if an older one is still valid. */
export function subjectEvidence(
  observations: Observation[],
  subject: EntityRef,
  now: number,
): Evidence {
  const latest = subjectObservations(observations, subject)[0]
  if (!latest) return { state: 'unobserved', latest: null }
  return { state: isCurrent(latest, now) ? latest.result : 'stale', latest }
}

const incidentRank: Record<IncidentSeverity, number> = { critical: 0, warning: 2, info: 5 }

// Healthy evidence needs no attention; never-observed subjects are a coverage gap instead.
function evidenceRank(evidence: Evidence): number | null {
  if (evidence.state === 'unhealthy') return 1
  if (evidence.state === 'degraded') return 3
  if (evidence.state === 'stale') return evidence.latest.result === 'healthy' ? 6 : 4
  return null
}

/**
 * Ordered by urgency: critical incidents, observed failures, warnings, degraded
 * results, expired problems, informational incidents, then expired healthy results.
 */
export function attentionItems(
  subjects: EntityRef[],
  observations: Observation[],
  incidents: RecordedIncident[],
  now: number,
): AttentionItem[] {
  const ranked: { item: AttentionItem; rank: number; at: string }[] = activeIncidents(
    incidents,
  ).map((incident) => ({
    item: { kind: 'incident', subject: incident.subject, incident },
    rank: incidentRank[incident.severity],
    at: incident.openedAt,
  }))
  for (const subject of subjects) {
    const evidence = subjectEvidence(observations, subject, now)
    const rank = evidenceRank(evidence)
    if (rank !== null && evidence.latest)
      ranked.push({
        item: { kind: 'evidence', subject, evidence },
        rank,
        at: evidence.latest.observedAt,
      })
  }
  return ranked
    .sort((a, b) => a.rank - b.rank || Date.parse(b.at) - Date.parse(a.at))
    .map(({ item }) => item)
}

export function evidenceCoverage(
  subjects: EntityRef[],
  observations: Observation[],
  now: number,
): EvidenceCoverage {
  const coverage = { current: 0, stale: 0, unobserved: 0 }
  for (const subject of subjects) {
    const { state } = subjectEvidence(observations, subject, now)
    if (state === 'unobserved' || state === 'stale') coverage[state] += 1
    else coverage.current += 1
  }
  return coverage
}

/** An empty attention list is only reassuring when every subject has current evidence. */
export function workspaceStatus(attention: AttentionItem[], coverage: EvidenceCoverage) {
  if (coverage.current + coverage.stale + coverage.unobserved === 0) return 'empty'
  if (attention.length > 0) return 'attention'
  if (coverage.unobserved > 0) return 'unobserved'
  return 'covered'
}
