/// <reference types="vite/client" />
// The window's side of the pending work contract; the native side is src-tauri/src/pending.rs.
// Both rank the same fixtures in contract/pending-work/, so the agent gate can never answer
// differently from Overview: changing a rule on one side breaks a test until the other side and the
// fixtures agree again.
import { describe, expect, it } from 'vitest'
import { pendingWork, type PendingItem } from './pending'

interface PendingCase {
  about: string
  now: string
  thisWorkstation: string | null
  archived: string[]
  localCopies: Parameters<typeof pendingWork>[0]
  installations: Parameters<typeof pendingWork>[1]
  githubRepositories: Parameters<typeof pendingWork>[2]
  expected: PendingItem[]
}

const cases = Object.entries(
  import.meta.glob<PendingCase>('../../../contract/pending-work/*.json', {
    eager: true,
    import: 'default',
  }),
).map(([file, value]) => [file.replace(/^.*\/(.+)\.json$/, '$1'), value] as const)

describe('the pending work contract', () => {
  it('has cases', () => {
    expect(cases.length).toBeGreaterThan(0)
  })

  it.each(cases)('%s', (_, item) => {
    expect(
      pendingWork(
        item.localCopies,
        item.installations,
        item.githubRepositories,
        item.thisWorkstation,
        item.archived,
        Date.parse(item.now),
      ),
    ).toEqual(item.expected)
  })
})
