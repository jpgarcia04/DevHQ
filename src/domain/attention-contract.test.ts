/// <reference types="vite/client" />
// The window's side of the attention contract; the native side is src-tauri/src/attention.rs.
// Both compute «what needs my attention» from the same fixtures in contract/attention/, so the
// agent gate can never answer differently from the window: changing a rule on one side breaks a
// test until the other side and the fixtures agree again.
import { describe, expect, it } from 'vitest'
import {
  attentionItems,
  evidenceCoverage,
  workspaceStatus,
  type EvidenceCoverage,
} from './evidence'
import type { EntityRef, Observation, RecordedIncident } from './models'

interface AttentionCase {
  about: string
  now: string
  subjects: EntityRef[]
  observations: Observation[]
  incidents: RecordedIncident[]
  expected: {
    attention: (
      | { kind: 'incident'; subject: EntityRef; incidentId: string }
      | { kind: 'evidence'; subject: EntityRef; state: string; observationId: string }
    )[]
    coverage: EvidenceCoverage
    status: ReturnType<typeof workspaceStatus>
  }
}

const cases = Object.entries(
  import.meta.glob<AttentionCase>('../../contract/attention/*.json', {
    eager: true,
    import: 'default',
  }),
).map(([file, value]) => [file.replace(/^.*\/(.+)\.json$/, '$1'), value] as const)

describe('the attention contract', () => {
  it('has cases', () => {
    expect(cases.length).toBeGreaterThan(0)
  })

  it.each(cases)('%s', (_, { now, subjects, observations, incidents, expected }) => {
    const at = Date.parse(now)
    const attention = attentionItems(subjects, observations, incidents, at)
    const coverage = evidenceCoverage(subjects, observations, at)
    expect(
      attention.map((item) =>
        item.kind === 'incident'
          ? { kind: item.kind, subject: item.subject, incidentId: item.incident.id }
          : {
              kind: item.kind,
              subject: item.subject,
              state: item.evidence.state,
              observationId: item.evidence.latest.id,
            },
      ),
    ).toEqual(expected.attention)
    expect(coverage).toEqual(expected.coverage)
    expect(workspaceStatus(attention, coverage)).toBe(expected.status)
  })
})
