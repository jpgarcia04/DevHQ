import { describe, expect, it } from 'vitest'
import {
  attentionItems,
  concernsApplication,
  evidenceCoverage,
  subjectEvidence,
  workspaceStatus,
} from './evidence'
import type { EntityRef, Observation, RecordedIncident } from './models'

const now = Date.parse('2026-09-30T12:00:00Z')
const at = (minutesFromNow: number) => new Date(now + minutesFromNow * 60_000).toISOString()
const app: EntityRef = { type: 'application', id: 'shop' }
const host: EntityRef = { type: 'resource', id: 'host' }
const spare: EntityRef = { type: 'resource', id: 'spare' }

function observation(
  subject: EntityRef,
  result: Observation['result'],
  observedMinutesAgo: number,
  validMinutes: number,
): Observation {
  return {
    id: `${subject.id}-${observedMinutesAgo}`,
    subject,
    result,
    summary: 'Checked by hand',
    source: 'manual',
    observedAt: at(-observedMinutesAgo),
    validUntil: at(validMinutes - observedMinutesAgo),
    recordedAt: at(-observedMinutesAgo),
  }
}

function incident(
  severity: RecordedIncident['severity'],
  status: RecordedIncident['status'] = 'open',
): RecordedIncident {
  return {
    id: `${severity}-${status}`,
    subject: app,
    title: `${severity} incident`,
    description: '',
    severity,
    status,
    source: 'manual',
    openedAt: at(-30),
    updatedAt: at(-30),
    resolvedAt: status === 'resolved' ? at(-5) : null,
  }
}

describe('evidence validity', () => {
  it('reports a subject without observations as unobserved, never healthy', () => {
    expect(subjectEvidence([], host, now)).toEqual({ state: 'unobserved', latest: null })
  })

  it('keeps a result while it is valid and expires it at the end of its validity', () => {
    const healthy = observation(host, 'healthy', 30, 60)
    expect(subjectEvidence([healthy], host, now).state).toBe('healthy')
    expect(subjectEvidence([healthy], host, now + 29 * 60_000).state).toBe('healthy')
    // Exactly at validUntil the observation no longer vouches for anything.
    expect(subjectEvidence([healthy], host, now + 30 * 60_000).state).toBe('stale')
  })

  it('keeps the failure visible as the last result of expired evidence', () => {
    const failure = observation(host, 'unhealthy', 120, 60)
    expect(subjectEvidence([failure], host, now)).toEqual({ state: 'stale', latest: failure })
  })

  it('lets the newest observation supersede an older, still valid one', () => {
    const olderFailure = observation(host, 'unhealthy', 90, 24 * 60)
    const newerHealthy = observation(host, 'healthy', 10, 60)
    const expiredHealthy = observation(host, 'healthy', 45, 30)
    expect(subjectEvidence([olderFailure, newerHealthy], host, now).state).toBe('healthy')
    expect(subjectEvidence([expiredHealthy, olderFailure], host, now)).toEqual({
      state: 'stale',
      latest: expiredHealthy,
    })
  })

  it('never borrows evidence from another subject', () => {
    expect(subjectEvidence([observation(host, 'healthy', 5, 60)], app, now).state).toBe(
      'unobserved',
    )
  })
})

describe('attention', () => {
  it('orders incidents and evidence by urgency and ignores healthy or resolved records', () => {
    const observations = [
      observation(host, 'unhealthy', 5, 60),
      observation(app, 'healthy', 200, 60),
      observation(spare, 'healthy', 5, 60),
    ]
    const incidents = [incident('info'), incident('critical'), incident('warning', 'resolved')]
    const items = attentionItems([app, host, spare], observations, incidents, now)
    expect(
      items.map((item) =>
        item.kind === 'incident'
          ? `incident:${item.incident.severity}`
          : `${item.subject.id}:${item.evidence.state}`,
      ),
    ).toEqual(['incident:critical', 'host:unhealthy', 'incident:info', 'shop:stale'])
  })

  it('ranks an expired failure above an expired healthy result', () => {
    const items = attentionItems(
      [app, host],
      [observation(app, 'healthy', 200, 60), observation(host, 'unhealthy', 300, 60)],
      [],
      now,
    )
    expect(items.map((item) => item.subject.id)).toEqual(['host', 'shop'])
  })

  it('does not treat missing coverage as a healthy workspace', () => {
    const subjects = [app, host]
    const partial = evidenceCoverage(subjects, [observation(app, 'healthy', 5, 60)], now)
    expect(partial).toEqual({ current: 1, stale: 0, unobserved: 1 })
    expect(workspaceStatus([], partial)).toBe('unobserved')
    const full = evidenceCoverage(
      subjects,
      [observation(app, 'healthy', 5, 60), observation(host, 'healthy', 5, 60)],
      now,
    )
    expect(workspaceStatus([], full)).toBe('covered')
    expect(workspaceStatus([], evidenceCoverage([], [], now))).toBe('empty')
    const expired = [observation(app, 'healthy', 200, 60)]
    expect(
      workspaceStatus(
        attentionItems([app], expired, [], now),
        evidenceCoverage([app], expired, now),
      ),
    ).toBe('attention')
  })

  it('relates resource records to the applications that use the resource', () => {
    expect(concernsApplication(host, 'shop', ['host'])).toBe(true)
    expect(concernsApplication(spare, 'shop', ['host'])).toBe(false)
    expect(concernsApplication(app, 'shop', [])).toBe(true)
  })
})
