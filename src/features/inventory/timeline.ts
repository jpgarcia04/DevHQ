import { useState } from 'react'
import { sameSubject } from '@/domain/evidence'
import type { EntityRef, RecordedEvent } from '@/domain/models'
import { automatedSince, mergeTimeline, windowCut } from './history'
import { inventoryError } from './inventory-api'
import { useInventory } from './inventory-context'

/** What a timeline loaded beyond its snapshot. */
interface Older {
  view: string
  /** Older automated events, and every automated event loaded when they were asked for. */
  events: RecordedEvent[]
  /** The oldest event of the last page: the timeline is complete from it on. */
  cursor: RecordedEvent | null
  pages: number
  more: boolean
  loading: boolean
  error: string | null
}

const nothing = (view: string): Older => ({
  view,
  events: [],
  cursor: null,
  pages: 0,
  more: true,
  loading: false,
  error: null,
})

/**
 * The events of a timeline over `subjects`, newest first, with the automated ones the
 * snapshot left out loaded on demand, a page at a time. `view` names the timeline: what was
 * loaded for one never carries over to another.
 *
 * `since` is the moment the timeline is complete from (null when it is complete): before it,
 * only manual records are loaded. Pages are anchored on an event, never counted, so new
 * results arriving meanwhile cause neither gaps nor duplicates.
 */
export function useTimeline(view: string, subjects: EntityRef[]) {
  const { snapshot, olderEvents } = useInventory()
  const [state, setState] = useState(() => nothing(view))
  const older = state.view === view ? state : nothing(view)
  const inView = (subject: EntityRef) => subjects.some((item) => sameSubject(item, subject))
  const own = (snapshot?.events ?? []).filter((event) => inView(event.subject))
  const history = snapshot?.automatedHistory ?? { limit: 0, truncated: [] }
  const since = !older.pages
    ? automatedSince(history, subjects)
    : older.more
      ? (older.cursor?.occurredAt ?? null)
      : null

  const loadOlder = async () => {
    const anchor = older.cursor ?? windowCut(own, history, subjects)
    if (!anchor) return
    // Only the subjects that were cut have automated events the snapshot left out.
    const cut = history.truncated.map((item) => item.subject).filter(inView)
    setState({ ...older, loading: true, error: null })
    try {
      const page = await olderEvents(cut, anchor.id)
      setState((current) => {
        if (current.view !== view) return current
        return {
          ...current,
          // Keep every automated event loaded now as well: a newer snapshot may drop it
          // from its window, which would open a gap.
          events: mergeTimeline(
            current.events,
            own.filter((event) => event.source === 'http').concat(page.events),
          ),
          cursor: page.events.at(-1) ?? current.cursor,
          pages: current.pages + 1,
          more: page.more,
          loading: false,
        }
      })
    } catch (cause) {
      setState((current) =>
        current.view === view
          ? { ...current, loading: false, error: inventoryError(cause) }
          : current,
      )
    }
  }

  return {
    events: older.events.length ? mergeTimeline(own, older.events) : own,
    since,
    limit: history.limit,
    loadOlder,
    loading: older.loading,
    error: older.error,
    /** Older results were asked for and none is left. */
    complete: older.pages > 0 && !older.more,
  }
}
