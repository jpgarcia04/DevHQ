import { sameSubject } from '@/domain/evidence'
import type { EntityRef, RecordSource } from '@/domain/models'
import type { AutomatedHistory } from './inventory-api'

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
