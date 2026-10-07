import { describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import type { ComponentType, ReactNode } from 'react'
import type { Observation, RecordedEvent, RecordedIncident } from '@/domain/models'
import { inventoryApi, type InventorySnapshot } from './inventory-api'
import { InventoryContext, useInventory, type InventoryContextValue } from './inventory-context'
import { InventoryProvider } from './inventory-provider'
import { RealOverviewPage } from './real-overview-page'
import { IncidentForm, ObservationForm } from './record-forms'
import { IncidentRecordList } from './record-lists'
import { RealActivityPage, RealIncidentsPage } from './records-pages'
import { inventoryContext, minutesFromNow, testNow } from './test-context'

function observation(
  resourceId: string,
  result: Observation['result'],
  observedMinutesAgo: number,
  validUntilMinutes: number,
): Observation {
  return {
    id: `obs-${resourceId}`,
    subject: { type: 'resource', id: resourceId },
    result,
    summary: `${resourceId} checked by hand`,
    source: 'manual',
    observedAt: minutesFromNow(-observedMinutesAgo),
    validUntil: minutesFromNow(validUntilMinutes),
    recordedAt: minutesFromNow(-observedMinutesAgo),
    http: null,
    agent: null,
  }
}

const openIncident: RecordedIncident = {
  id: 'inc-1',
  subject: { type: 'application', id: 'shop' },
  title: 'Checkout returns 502',
  description: 'Seen while paying an order.',
  severity: 'critical',
  status: 'open',
  source: 'manual',
  openedAt: minutesFromNow(-20),
  updatedAt: minutesFromNow(-20),
  resolvedAt: null,
  agent: null,
}

const events: RecordedEvent[] = [
  {
    id: 'evt-2',
    subject: { type: 'application', id: 'shop' },
    kind: 'deployment',
    outcome: 'success',
    title: 'Deployed 1.5 by hand',
    detail: 'Copied the build to the server.',
    source: 'manual',
    observationId: null,
    incidentId: null,
    occurredAt: minutesFromNow(-15),
    recordedAt: minutesFromNow(-15),
    agent: null,
  },
  {
    id: 'evt-1',
    subject: { type: 'resource', id: 'db-host' },
    kind: 'observation',
    outcome: 'failure',
    title: 'Observation: Unhealthy',
    detail: 'db-host checked by hand',
    source: 'manual',
    observationId: 'obs-db-host',
    incidentId: null,
    occurredAt: minutesFromNow(-10),
    // Typed in well after it happened: the timeline says so.
    recordedAt: minutesFromNow(-1),
    agent: null,
  },
]

const snapshot: InventorySnapshot = {
  applications: [
    {
      id: 'shop',
      name: 'Shop',
      description: '',
      lifecycle: 'production',
      version: null,
      components: [{ id: 'api', name: 'API', kind: 'backend', resourceId: 'db-host' }],
      references: [],
    },
  ],
  resources: ['spare', 'web-host', 'db-host'].map((id) => ({
    id,
    name: { spare: 'Spare box', 'web-host': 'Web host', 'db-host': 'Database host' }[id]!,
    kind: 'server' as const,
    description: '',
    operatingSystem: '',
    references: [],
  })),
  sharedRepositories: [],
  observations: [
    observation('db-host', 'unhealthy', 10, 50),
    observation('web-host', 'healthy', 180, -120),
  ],
  incidents: [],
  events: [],
  httpChecks: [],
  changes: [],
  changesSince: null,
  automatedHistory: { limit: 200, truncated: [] },
}

function renderWith(ui: ReactNode, value: InventoryContextValue, path = '/') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <InventoryContext.Provider value={value}>{ui}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

function rowFor(container: HTMLElement, name: string) {
  return within(within(container).getByRole('link', { name }).closest('li')!)
}

function SelectRealInventory() {
  const { setMode } = useInventory()
  return <button onClick={() => setMode('real')}>Select real inventory</button>
}

describe('real operational records', () => {
  it('distinguishes never observed, expired and failing evidence with its provenance', () => {
    renderWith(<RealOverviewPage />, inventoryContext({ snapshot }))
    const coverage = screen.getByRole('region', { name: 'Evidence coverage' })
    expect(rowFor(coverage, 'Spare box').getByText('Not observed')).toBeInTheDocument()
    expect(rowFor(coverage, 'Spare box').getByText('Never observed')).toBeInTheDocument()
    expect(rowFor(coverage, 'Web host').getByText('Observation expired')).toBeInTheDocument()
    expect(rowFor(coverage, 'Web host').getByText('Expired 2 hours ago')).toBeInTheDocument()
    expect(rowFor(coverage, 'Database host').getByText('Unhealthy')).toBeInTheDocument()
    expect(rowFor(coverage, 'Database host').getByText('Observed 10 min ago')).toBeInTheDocument()
    // The expired healthy result must not be presented as healthy.
    expect(within(coverage).queryByText('Healthy')).not.toBeInTheDocument()

    const attention = screen.getByRole('region', { name: /Needs attention/ })
    expect(within(attention).getByRole('link', { name: 'Database host' })).toBeInTheDocument()
    expect(within(attention).getByRole('link', { name: 'Web host' })).toBeInTheDocument()
    expect(within(attention).queryByRole('link', { name: 'Spare box' })).not.toBeInTheDocument()
    expect(within(attention).getAllByText('Manual entry')).toHaveLength(2)
    expect(within(attention).getByText(/Last result:/)).toHaveTextContent(
      'Last result: Healthy. web-host checked by hand',
    )
    expect(screen.getByText('2 items need your attention.')).toBeInTheDocument()
  })

  it('shows only recorded activity, attributed to its source', () => {
    renderWith(<RealActivityPage />, inventoryContext({ snapshot: { ...snapshot, events } }))
    expect(screen.getByText('Deployed 1.5 by hand')).toBeInTheDocument()
    expect(screen.getByText('Observation: Unhealthy')).toBeInTheDocument()
    expect(screen.getAllByText('Manual entry')).toHaveLength(2)
    expect(screen.getAllByText(/^recorded/)).toHaveLength(1)
    // Demo events never leak into the real timeline.
    expect(screen.queryByText('Backup verified')).not.toBeInTheDocument()
    expect(screen.queryByText('Deployment failed')).not.toBeInTheDocument()
  })

  it('mixes what an agent reported into activity and incidents, naming the agent', async () => {
    const user = userEvent.setup()
    const byAgent: RecordedEvent = {
      ...events[0]!,
      id: 'evt-agent',
      title: 'Deployed 1.6',
      source: 'agent',
      agent: 'claude-code',
      occurredAt: minutesFromNow(-5),
      recordedAt: minutesFromNow(-5),
    }
    const opened: RecordedIncident = { ...openIncident, source: 'agent', agent: 'claude-code' }
    const { unmount } = renderWith(
      <RealActivityPage />,
      inventoryContext({ snapshot: { ...snapshot, events: [byAgent, ...events] } }),
    )
    // One timeline: the agent's entry sits among the person's, with its own provenance.
    expect(screen.getByText('Deployed 1.6')).toBeInTheDocument()
    expect(screen.getByText('Deployed 1.5 by hand')).toBeInTheDocument()
    expect(screen.getAllByText('Manual entry')).toHaveLength(2)
    const tag = screen.getByText('Agent · claude-code')
    expect(tag.closest('[title]')).toHaveAttribute(
      'title',
      expect.stringContaining('DevHQ cannot verify it'),
    )
    unmount()
    renderWith(
      <RealIncidentsPage />,
      inventoryContext({ snapshot: { ...snapshot, incidents: [opened] } }),
    )
    await user.click(screen.getByText('Checkout returns 502'))
    expect(screen.getByText('Agent · claude-code')).toBeInTheDocument()
    expect(screen.queryByText('Manual entry')).not.toBeInTheDocument()
  })

  it('mixes configuration changes into the timeline with who made them, kept when a system is gone', async () => {
    const user = userEvent.setup()
    const changes: InventorySnapshot['changes'] = [
      {
        id: 'c1',
        subject: { type: 'application', id: 'shop' },
        subjectName: 'Shop',
        source: 'agent',
        agent: 'claude-code',
        action: 'updated',
        summary: 'Updated application «Shop»: version «1.4» → «1.5»',
        changes: [{ field: 'version', before: '1.4', after: '1.5' }],
        occurredAt: minutesFromNow(-12),
      },
      {
        id: 'c2',
        subject: { type: 'resource', id: 'old-box' },
        subjectName: 'Old box',
        source: 'manual',
        agent: null,
        action: 'deleted',
        summary: 'Deleted resource «Old box», which had no history',
        changes: [{ field: 'name', before: 'Old box', after: null }],
        occurredAt: minutesFromNow(-30),
      },
    ]
    const { unmount } = renderWith(
      <RealActivityPage />,
      inventoryContext({ snapshot: { ...snapshot, events, changes } }),
    )
    // In time order among the person's entries: after evt-1 (10 min ago), before evt-2 (15).
    const titles = screen
      .getAllByRole('listitem')
      .map((item) => item.querySelector('p')?.textContent)
    expect(titles).toEqual([
      'Observation: Unhealthy',
      'Updated application «Shop»: version «1.4» → «1.5»',
      'Deployed 1.5 by hand',
      'Deleted resource «Old box», which had no history',
    ])
    expect(screen.getAllByText('Change')).toHaveLength(2)
    // The agent's change says which agent; the person's, that it was typed by hand.
    expect(screen.getAllByText('Agent · claude-code')).toHaveLength(1)
    const deletion = screen
      .getAllByRole('listitem')
      .find((item) => item.textContent?.includes('Deleted resource «Old box»'))!
    expect(within(deletion).getByText('Manual entry')).toBeInTheDocument()
    // A system that is gone keeps the name the change gave it, without a link that leads nowhere.
    expect(screen.getByText('Old box · no longer in the inventory')).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: /Old box/ })).not.toBeInTheDocument()
    // One system's activity holds its own changes only.
    await user.selectOptions(
      screen.getByRole('combobox', { name: 'Filter activity by system' }),
      'Shop',
    )
    expect(
      screen.getByText('Updated application «Shop»: version «1.4» → «1.5»'),
    ).toBeInTheDocument()
    expect(screen.queryByText(/Old box/)).not.toBeInTheDocument()
    unmount()

    // Changes alone are activity too, and the overview's recent activity shows them.
    renderWith(
      <RealActivityPage />,
      inventoryContext({ snapshot: { ...snapshot, events: [], changes } }),
    )
    expect(screen.queryByRole('heading', { name: 'No activity recorded' })).not.toBeInTheDocument()
    expect(screen.getByText('Deleted resource «Old box», which had no history')).toBeInTheDocument()
  })

  it('shows an agent’s changes among the overview’s recent activity', () => {
    renderWith(
      <RealOverviewPage />,
      inventoryContext({
        snapshot: {
          ...snapshot,
          changes: [
            {
              id: 'c1',
              subject: { type: 'resource', id: 'spare' },
              subjectName: 'Spare box',
              source: 'agent',
              agent: 'codex',
              action: 'created',
              summary: 'Added resource «Spare box» (server)',
              changes: null,
              occurredAt: minutesFromNow(-3),
            },
          ],
        },
      }),
    )
    const recent = screen.getByRole('region', { name: 'Recent activity' })
    expect(within(recent).getByText('Added resource «Spare box» (server)')).toBeInTheDocument()
    expect(within(recent).getByText('Agent · codex')).toBeInTheDocument()
    expect(within(recent).queryByText('No activity recorded yet.')).not.toBeInTheDocument()
  })

  it('says from when configuration changes are complete, and nothing when they always were', () => {
    const changes: InventorySnapshot['changes'] = [
      {
        id: 'c1',
        subject: { type: 'application', id: 'shop' },
        subjectName: 'Shop',
        source: 'manual',
        agent: null,
        action: 'updated',
        summary: 'Updated application «Shop»: lifecycle testing → production',
        changes: [{ field: 'lifecycle', before: 'testing', after: 'production' }],
        occurredAt: minutesFromNow(-5),
      },
    ]
    const note = /Earlier changes made in the window left no trace/
    const { unmount } = renderWith(
      <RealActivityPage />,
      inventoryContext({ snapshot: { ...snapshot, events, changes, changesSince: null } }),
    )
    expect(screen.queryByText(note)).not.toBeInTheDocument()
    unmount()
    renderWith(
      <RealActivityPage />,
      inventoryContext({
        snapshot: { ...snapshot, events, changes, changesSince: minutesFromNow(-60 * 24) },
      }),
    )
    expect(screen.getByText(note)).toBeInTheDocument()
  })

  it('lists recorded incidents with their affected system', async () => {
    const user = userEvent.setup()
    renderWith(
      <RealIncidentsPage />,
      inventoryContext({ snapshot: { ...snapshot, incidents: [openIncident] } }),
    )
    expect(screen.getByText('1 active incident')).toBeInTheDocument()
    await user.click(screen.getByText('Checkout returns 502'))
    expect(screen.getByRole('link', { name: 'Shop' })).toBeInTheDocument()
    expect(screen.getByText('Manual entry')).toBeInTheDocument()
    expect(screen.queryByText('ClipOps deployment failed')).not.toBeInTheDocument()
  })

  it.each([
    ['Incidents', RealIncidentsPage, 'No incidents recorded'],
    ['Activity', RealActivityPage, 'No activity recorded'],
  ] as [string, ComponentType, string][])(
    'reports a failed %s load as an error and recovers on retry',
    async (_, Page, emptyTitle) => {
      const user = userEvent.setup()
      const list = vi
        .spyOn(inventoryApi, 'list')
        .mockRejectedValueOnce({
          code: 'database_error',
          message: 'Inventory database error: locked',
        })
        .mockResolvedValueOnce({ ...snapshot, observations: [] })
      render(
        <MemoryRouter>
          <InventoryProvider>
            <SelectRealInventory />
            <Page />
          </InventoryProvider>
        </MemoryRouter>,
      )
      await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
      expect(
        await screen.findByRole('heading', { name: 'Inventory unavailable' }),
      ).toBeInTheDocument()
      expect(screen.getByText(/locked/)).toBeInTheDocument()
      expect(screen.queryByText(emptyTitle)).not.toBeInTheDocument()
      expect(screen.queryByText(/active incidents/)).not.toBeInTheDocument()
      await user.click(screen.getByRole('button', { name: 'Retry' }))
      expect(await screen.findByRole('heading', { name: emptyTitle })).toBeInTheDocument()
      expect(list).toHaveBeenCalledTimes(2)
      list.mockRestore()
    },
  )

  it('records a manual observation for the subject in context', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot })
    const onDone = vi.fn()
    renderWith(
      <ObservationForm subject={{ type: 'resource', id: 'db-host' }} onDone={onDone} />,
      value,
    )
    expect(screen.getByText(/Manual entry: what you checked yourself/)).toBeInTheDocument()
    expect(screen.queryByRole('combobox', { name: 'System' })).not.toBeInTheDocument()
    await user.selectOptions(screen.getByRole('combobox', { name: 'Result' }), 'Unhealthy')
    await user.type(screen.getByRole('textbox', { name: 'What did you observe?' }), 'Disk at 97%')
    await user.selectOptions(screen.getByRole('combobox', { name: 'Valid for' }), '1 hour')
    await user.click(screen.getByRole('button', { name: 'Save observation' }))
    expect(value.recordObservation).toHaveBeenCalledWith({
      subject: { type: 'resource', id: 'db-host' },
      result: 'unhealthy',
      summary: 'Disk at 97%',
      observedAt: new Date(testNow).toISOString(),
      validForMinutes: 60,
    })
    expect(onDone).toHaveBeenCalledOnce()
  })

  it('keeps the form open and shows a reference rejected by SQLite', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({
      snapshot,
      openIncident: vi.fn().mockRejectedValue({
        code: 'invalid_input',
        message: 'The resource db-host does not exist. Choose an existing resource.',
      }),
    })
    const onDone = vi.fn()
    renderWith(<IncidentForm onDone={onDone} />, value)
    await user.selectOptions(screen.getByRole('combobox', { name: 'System' }), 'Database host')
    await user.type(screen.getByRole('textbox', { name: 'Title' }), 'Replica lagging')
    await user.click(screen.getByRole('button', { name: 'Open incident' }))
    expect(value.openIncident).toHaveBeenCalledWith(
      expect.objectContaining({
        subject: { type: 'resource', id: 'db-host' },
        severity: 'warning',
      }),
    )
    expect(await screen.findByRole('alert')).toHaveTextContent('does not exist')
    expect(onDone).not.toHaveBeenCalled()
    expect(screen.getByRole('form', { name: 'Open incident' })).toBeInTheDocument()
  })

  it('changes an incident status with a note', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot: { ...snapshot, incidents: [openIncident] } })
    renderWith(<IncidentRecordList items={[openIncident]} />, value)
    await user.click(screen.getByText('Checkout returns 502'))
    expect(screen.getByRole('button', { name: 'Start investigating' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Reopen' })).not.toBeInTheDocument()
    await user.type(
      screen.getByRole('textbox', { name: 'Status note (optional)' }),
      'Rolled back to 1.4',
    )
    await user.click(screen.getByRole('button', { name: 'Resolve' }))
    expect(value.updateIncidentStatus).toHaveBeenCalledWith({
      id: 'inc-1',
      status: 'resolved',
      note: 'Rolled back to 1.4',
    })
  })
})
