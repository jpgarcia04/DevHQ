import { sameSubject } from '@/domain/evidence'
import type { EntityRef, RecordedEvent, RecordSource } from '@/domain/models'
import type { AutomatedHistory } from './inventory-api'

/** One timeline row: an event, or consecutive automated results that said the same thing. */
export type TimelineRow =
  { kind: 'event'; event: RecordedEvent } | { kind: 'repeats'; events: RecordedEvent[] }

/** Fewer repeats than this are clearer one by one. */
export const minimumRepeats = 3

// What a result said, without how long it took: "HTTP 500, expected 200, in 112 ms" and
// "Timed out after 1003 ms" become "HTTP 500, expected 200" and "Timed out".
const withoutTiming = (detail: string) => detail.replace(/(?:,? in| after) \d+ ms$/, '')

const repeats = (a: RecordedEvent, b: RecordedEvent) =>
  a.source === 'http' &&
  b.source === 'http' &&
  sameSubject(a.subject, b.subject) &&
  a.title === b.title &&
  withoutTiming(a.detail) === withoutTiming(b.detail)

/**
 * Folds runs of automated results of one system that said the same thing (the same status or
 * the same failure, whatever their timing) into a single row, so supervision cannot bury what
 * changed. Only adjacent events fold: anything in between, from any system,
 * ends the run, so the timeline keeps its order.
 */
export function timelineRows(events: RecordedEvent[]): TimelineRow[] {
  const runs: RecordedEvent[][] = []
  for (const event of events) {
    const run = runs.at(-1)
    if (run && repeats(run[0]!, event)) run.push(event)
    else runs.push([event])
  }
  return runs.flatMap((run): TimelineRow[] =>
    run.length >= minimumRepeats
      ? [{ kind: 'repeats', events: run }]
      : run.map((event) => ({ kind: 'event', event })),
  )
}

/**
 * The moment from which the automated results of every given subject are loaded, or null if
 * none was left out. A view that merges subjects is complete only from the latest of their
 * moments on: before it, one subject's automated results are missing, so showing another's
 * would suggest the first had none.
 */
export function automatedSince(history: AutomatedHistory, subjects: EntityRef[]): string | null {
  let since: string | null = null
  for (const item of history.truncated) {
    if (!subjects.some((subject) => sameSubject(subject, item.subject))) continue
    if (since === null || Date.parse(item.since) > Date.parse(since)) since = item.since
  }
  return since
}

/** Reported records (manual or by an agent) always belong; automated ones only from `since` on. */
export const withinWindow = (source: RecordSource, time: string, since: string | null) =>
  since === null || source !== 'http' || Date.parse(time) >= Date.parse(since)

/** Whether a record is older than the moment a view is complete from. */
export const beforeWindow = (time: string, since: string | null) =>
  since !== null && Date.parse(time) < Date.parse(since)

const newestFirst = (a: RecordedEvent, b: RecordedEvent) =>
  Date.parse(b.occurredAt) - Date.parse(a.occurredAt) ||
  Date.parse(b.recordedAt) - Date.parse(a.recordedAt)

/**
 * One timeline from two lists of events: each event once, newest first. Events at the same
 * moment keep the order they arrive in, which within each list is the native one.
 */
export function mergeTimeline(events: RecordedEvent[], more: RecordedEvent[]): RecordedEvent[] {
  const seen = new Set<string>()
  return [...events, ...more]
    .filter((event) => !seen.has(event.id) && Boolean(seen.add(event.id)))
    .sort(newestFirst)
}

/**
 * Where a timeline over `subjects` stops being complete: among the subjects whose automated
 * results the snapshot cut, the one cut latest, at its oldest loaded result. Every automated
 * event after it, of every subject, is loaded, so older ones are read from it on and no
 * system is skipped. Null when nothing was cut.
 */
export function windowCut(
  events: RecordedEvent[],
  history: AutomatedHistory,
  subjects: EntityRef[],
): RecordedEvent | null {
  let cut: number | null = null
  for (const { subject } of history.truncated) {
    if (!subjects.some((item) => sameSubject(item, subject))) continue
    // Newest first: its oldest loaded result is its last one.
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const event = events[index]!
      if (event.source !== 'http' || !sameSubject(event.subject, subject)) continue
      if (cut === null || index < cut) cut = index
      break
    }
  }
  return cut === null ? null : events[cut]!
}
