import { sameSubject } from '@/domain/evidence'
import type { EntityRef, RecordedEvent, RecordSource } from '@/domain/models'
import type { AutomatedHistory } from './inventory-api'

/** One timeline row: an event, or consecutive automated results that said the same thing. */
export type TimelineRow =
  { kind: 'event'; event: RecordedEvent } | { kind: 'repeats'; events: RecordedEvent[] }

/** Fewer repeats than this are clearer one by one. */
export const minimumRepeats = 3

const repeats = (a: RecordedEvent, b: RecordedEvent) =>
  a.source === 'http' &&
  b.source === 'http' &&
  sameSubject(a.subject, b.subject) &&
  a.title === b.title

/**
 * Folds runs of identical automated results of one system into a single row, so supervision
 * cannot bury what changed. Only adjacent events fold: anything in between, from any system,
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

/** Manual records always belong; automated ones only from `since` on. */
export const withinWindow = (source: RecordSource, time: string, since: string | null) =>
  since === null || source === 'manual' || Date.parse(time) >= Date.parse(since)

/** Whether a record is older than the moment a view is complete from. */
export const beforeWindow = (time: string, since: string | null) =>
  since !== null && Date.parse(time) < Date.parse(since)
