import { describe, expect, it } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type { ReactNode } from 'react'
import type { EntityRef, Observation, RecordedEvent, RecordSource } from '@/domain/models'
import { formatDate, formatDateTime } from '@/shared/time'
import { ObservationHistory } from './evidence'
import { automatedSince, timelineRows, withinWindow } from './history'
import type { InventorySnapshot } from './inventory-api'
import { InventoryContext, type InventoryContextValue } from './inventory-context'
import { RealApplicationDetailPage, RealOverviewPage } from './real-pages'
import { EventRecordList } from './record-lists'
import { RealActivityPage } from './records-pages'
import { inventoryContext, minutesFromNow } from './test-context'

const web: EntityRef = { type: 'resource', id: 'web-host' }
const db: EntityRef = { type: 'resource', id: 'db-host' }
const shop: EntityRef = { type: 'application', id: 'shop' }
// Supervision left web-host's automated results before this moment in SQLite.
const since = minutesFromNow(-60)

function observation(
  subject: EntityRef,
  source: RecordSource,
  minutesAgo: number,
  summary: string,
): Observation {
  return {
    id: `obs-${subject.id}-${minutesAgo}`,
    subject,
    result: 'healthy',
    summary,
    source,
    observedAt: minutesFromNow(-minutesAgo),
    validUntil: minutesFromNow(-minutesAgo + 15),
    recordedAt: minutesFromNow(-minutesAgo),
    http:
      source === 'http'
        ? {
            url: 'https://web.example/health',
            expectedStatus: 200,
            status: 200,
            latencyMs: 12,
            error: null,
            redirects: [],
          }
        : null,
  }
}

function event(
  subject: EntityRef,
  source: RecordSource,
  minutesAgo: number,
  title: string,
): RecordedEvent {
  return {
    id: `evt-${subject.id}-${minutesAgo}`,
    subject,
    kind: source === 'http' ? 'observation' : 'note',
    outcome: 'info',
    title,
    detail: '',
    source,
    observationId: source === 'http' ? `obs-${subject.id}-${minutesAgo}` : null,
    incidentId: null,
    occurredAt: minutesFromNow(-minutesAgo),
    recordedAt: minutesFromNow(-minutesAgo),
  }
}

const snapshot: InventorySnapshot = {
  applications: [
    {
      id: 'shop',
      name: 'Shop',
      description: '',
      lifecycle: 'production',
      version: null,
      components: [{ id: 'api', name: 'API', kind: 'backend', resourceId: 'web-host' }],
    },
  ],
  resources: [
    { id: 'web-host', name: 'Web host', kind: 'server', description: '', operatingSystem: '' },
    { id: 'db-host', name: 'Database host', kind: 'server', description: '', operatingSystem: '' },
  ],
  observations: [
    observation(web, 'http', 10, 'Run at -10'),
    observation(web, 'http', 30, 'Run at -30'),
    observation(web, 'http', 60, 'Run at -60'),
    observation(web, 'manual', 90, 'Checked by hand at -90'),
  ],
  incidents: [],
  events: [
    event(web, 'http', 10, 'Web run at -10'),
    event(web, 'http', 30, 'Web run at -30'),
    event(web, 'http', 60, 'Web run at -60'),
    event(web, 'manual', 90, 'Web note at -90'),
    // Database host keeps all its results, but merged with web-host they would suggest that
    // web-host had none at that time.
    event(db, 'http', 120, 'Database run at -120'),
    event(db, 'manual', 200, 'Database note at -200'),
    event(db, 'manual', 2000, 'Database note the day before'),
    // A day with nothing but automated results from before the window: no empty section.
    event(db, 'http', 3500, 'Database run two days before'),
  ],
  httpChecks: [],
  automatedHistory: { limit: 200, truncated: [{ subject: web, since }] },
}

function renderWith(ui: ReactNode, value: InventoryContextValue, path = '/') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <InventoryContext.Provider value={value}>{ui}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

const note =
  /^Automated HTTP check results before .+ are kept in the local database but not loaded: DevHQ loads the latest 200 per system\. Manual records are always complete\.$/
const boundaryText = `Before ${formatDateTime(since)} · manual records only`

/** Matches an element by its whole text, which a <time> inside splits into several nodes. */
const whole = (tag: string, text: RegExp | string) => (_: string, element: Element | null) =>
  element?.tagName === tag &&
  (typeof text === 'string' ? element.textContent === text : text.test(element.textContent ?? ''))
const theNote = whole('P', note)
const theBoundary = whole('LI', boundaryText)

/** The text of each list item, in order. */
const itemTexts = (container: HTMLElement) =>
  within(container)
    .getAllByRole('listitem')
    .map((item) => item.textContent ?? '')

describe('the automated history window', () => {
  it('is complete from the latest moment any merged subject was cut at', () => {
    const history = {
      limit: 200,
      truncated: [
        { subject: web, since },
        { subject: db, since: minutesFromNow(-30) },
      ],
    }
    expect(automatedSince(history, [web])).toBe(since)
    expect(automatedSince(history, [web, db])).toBe(minutesFromNow(-30))
    expect(automatedSince(history, [shop])).toBeNull()
    expect(withinWindow('http', minutesFromNow(-61), since)).toBe(false)
    expect(withinWindow('http', since, since)).toBe(true)
    expect(withinWindow('manual', minutesFromNow(-6000), since)).toBe(true)
    expect(withinWindow('http', minutesFromNow(-6000), null)).toBe(true)
  })

  it('says what an observation history leaves out and where it stops being complete', () => {
    renderWith(<ObservationHistory subject={web} />, inventoryContext({ snapshot }))
    expect(screen.getByText(theNote)).toBeInTheDocument()
    const items = itemTexts(screen.getByRole('list'))
    expect(items).toHaveLength(5)
    expect(items[2]).toContain('Run at -60')
    expect(items[3]).toBe(boundaryText)
    expect(items[4]).toContain('Checked by hand at -90')
  })

  it('needs no note for a subject whose whole history is loaded', () => {
    renderWith(<ObservationHistory subject={db} />, inventoryContext({ snapshot }))
    expect(screen.queryByText(theNote)).not.toBeInTheDocument()
  })

  it('keeps merged activity honest before the window and lifts it for a complete subject', async () => {
    const user = userEvent.setup()
    renderWith(<RealActivityPage />, inventoryContext({ snapshot }))
    expect(screen.getByText(theNote)).toBeInTheDocument()
    // Loaded, but older than the moment the merged view is complete from.
    expect(screen.queryByText('Database run at -120')).not.toBeInTheDocument()
    const today = screen.getByRole('region', { name: formatDate(minutesFromNow(-10)) })
    const items = itemTexts(today)
    const position = (text: string) => items.findIndex((item) => item.includes(text))
    expect(items).toHaveLength(6)
    expect(position('Web run at -60')).toBe(2)
    expect(items[3]).toBe(boundaryText)
    expect(position('Web note at -90')).toBe(4)
    expect(position('Database note at -200')).toBe(5)
    expect(within(today).getByText(theBoundary)).toBeInTheDocument()
    const dayBefore = formatDate(minutesFromNow(-2000))
    expect(
      screen.getByRole('heading', { name: `${dayBefore} · manual records only` }),
    ).toBeInTheDocument()
    expect(
      screen.queryByRole('region', { name: formatDate(minutesFromNow(-3500)) }),
    ).not.toBeInTheDocument()

    // Database host alone is complete: all its loaded activity shows, without a note.
    await user.selectOptions(screen.getByLabelText('Filter activity by system'), 'resource:db-host')
    expect(screen.getByText('Database run at -120')).toBeInTheDocument()
    expect(screen.getByText('Database run two days before')).toBeInTheDocument()
    expect(screen.queryByText(theNote)).not.toBeInTheDocument()
    expect(screen.queryByText(/manual records only/)).not.toBeInTheDocument()
  })

  it('applies to an application’s activity through the resources it runs on', () => {
    renderWith(
      <Routes>
        <Route path="/applications/:applicationId" element={<RealApplicationDetailPage />} />
      </Routes>,
      inventoryContext({ snapshot }),
      '/applications/shop?tab=activity',
    )
    const panel = screen.getByRole('tabpanel')
    expect(within(panel).getByText(theNote)).toBeInTheDocument()
    expect(within(panel).getByText(theBoundary)).toBeInTheDocument()
    expect(within(panel).getByText('Web note at -90')).toBeInTheDocument()
  })
})

describe('repeated automated results', () => {
  const run = (subject: EntityRef, minutesAgo: number, title = 'HTTP check: Healthy') => ({
    ...event(subject, 'http', minutesAgo, title),
    detail: `HTTP 200 as expected, run ${minutesAgo} min ago`,
  })
  const shape = (events: RecordedEvent[]) =>
    timelineRows(events).map((row) =>
      row.kind === 'event' ? row.event.id : row.events.map((item) => item.id),
    )

  it('fold only when three or more adjacent results of one system say the same', () => {
    const deployment = event(web, 'manual', 22, 'Deployed 2.0')
    expect(
      shape([
        run(web, 5),
        run(web, 10),
        run(web, 15),
        run(web, 20),
        deployment,
        run(web, 25),
        run(web, 30),
        run(db, 35),
        run(db, 40),
        run(db, 45, 'HTTP check: Unhealthy'),
        run(db, 50),
        run(db, 55),
        run(db, 60),
      ]),
    ).toEqual([
      ['evt-web-host-5', 'evt-web-host-10', 'evt-web-host-15', 'evt-web-host-20'],
      // Anything in between ends a run: the order of the timeline is kept.
      'evt-web-host-22',
      'evt-web-host-25',
      'evt-web-host-30',
      'evt-db-host-35',
      'evt-db-host-40',
      'evt-db-host-45',
      ['evt-db-host-50', 'evt-db-host-55', 'evt-db-host-60'],
    ])
    // Manual records never fold, however alike.
    expect(
      shape([
        event(web, 'manual', 1, 'Note'),
        event(web, 'manual', 2, 'Note'),
        event(web, 'manual', 3, 'Note'),
      ]),
    ).toHaveLength(3)
  })

  it('shows a folded run as one row that opens to every result', async () => {
    const user = userEvent.setup()
    const events = [run(web, 5), run(web, 10), run(web, 15), run(web, 20)]
    renderWith(
      <EventRecordList foldRepeats timeOnly items={events} />,
      inventoryContext({ snapshot }),
    )
    const timeline = screen.getAllByRole('list')[0]!
    expect(within(timeline).getAllByRole('listitem')).toHaveLength(1)
    expect(screen.getByText('4 consecutive results')).toBeInTheDocument()
    expect(screen.getByText('Latest: HTTP 200 as expected, run 5 min ago')).toBeInTheDocument()
    expect(screen.queryByText('HTTP 200 as expected, run 20 min ago')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Show all 4' }))
    const all = screen.getByRole('list', { name: '4 consecutive results' })
    expect(within(all).getAllByRole('listitem')).toHaveLength(4)
    expect(within(all).getByText('HTTP 200 as expected, run 20 min ago')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Hide them' })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
  })

  it('keep what changed among the overview’s recent activity', () => {
    const runs = [5, 10, 15, 20, 25, 30].map((minutesAgo) => run(web, minutesAgo))
    renderWith(
      <RealOverviewPage />,
      inventoryContext({
        snapshot: {
          ...snapshot,
          events: [
            ...runs,
            event(web, 'manual', 40, 'Deployed 2.0'),
            event(db, 'manual', 50, 'Old'),
          ],
          automatedHistory: { limit: 200, truncated: [] },
        },
      }),
    )
    const recent = within(screen.getByRole('region', { name: 'Recent activity' }))
    expect(recent.getByText('6 consecutive results')).toBeInTheDocument()
    expect(recent.getByText('Deployed 2.0')).toBeInTheDocument()
    expect(recent.getByText('Old')).toBeInTheDocument()
  })
})
