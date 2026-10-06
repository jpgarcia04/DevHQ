import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import type { ReactNode } from 'react'
import type { EntityRef, Observation } from '@/domain/models'
import { formatTime } from '@/shared/time'
import { HttpCheckPanel } from './http-check'
import {
  inventoryApi,
  type HttpCheck,
  type InventorySnapshot,
  type MonitorCheckStatus,
  type MonitorStatus,
} from './inventory-api'
import { InventoryContext, useInventory, type InventoryContextValue } from './inventory-context'
import { InventoryProvider } from './inventory-provider'
import { RealInfrastructurePage } from './real-infrastructure-page'
import { RealOverviewPage } from './real-overview-page'
import { inventoryContext, minutesFromNow } from './test-context'

const web: EntityRef = { type: 'resource', id: 'web-host' }
const db: EntityRef = { type: 'resource', id: 'db-host' }

function check(subject: EntityRef, overrides: Partial<HttpCheck> = {}): HttpCheck {
  return {
    subject,
    url: 'https://web.example/health',
    expectedStatus: 200,
    timeoutSeconds: 5,
    validForMinutes: 60,
    updatedAt: minutesFromNow(-60),
    monitor: 'active',
    monitorIntervalMinutes: 15,
    ...overrides,
  }
}

function running(
  subject: EntityRef,
  overrides: Partial<MonitorCheckStatus> = {},
): MonitorCheckStatus {
  return {
    subject,
    running: false,
    lastRunAt: minutesFromNow(-3),
    nextRunAt: minutesFromNow(12),
    lastError: null,
    ...overrides,
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
      components: [],
    },
  ],
  resources: [
    { id: 'web-host', name: 'Web host', kind: 'server', description: '', operatingSystem: '' },
    { id: 'db-host', name: 'Database host', kind: 'server', description: '', operatingSystem: '' },
  ],
  observations: [],
  incidents: [],
  events: [],
  httpChecks: [
    check(web),
    check(db, { url: 'https://db.example/ready', monitor: 'paused', monitorIntervalMinutes: 30 }),
  ],
  automatedHistory: { limit: 200, truncated: [] },
}

const monitor = (checks: MonitorCheckStatus[], recorded = 0): MonitorStatus => ({
  checks,
  error: null,
  recorded,
})

function renderWith(ui: ReactNode, value: InventoryContextValue) {
  return render(
    <MemoryRouter>
      <InventoryContext.Provider value={value}>{ui}</InventoryContext.Provider>
    </MemoryRouter>,
  )
}

describe('supervision of a check', () => {
  it('shows an active check’s interval, last and next run, and pauses it', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({
      snapshot,
      monitor: monitor([running(web), running(db, { nextRunAt: null })]),
    })
    renderWith(<HttpCheckPanel subject={web} name="Web host" />, value)
    const group = within(screen.getByRole('group', { name: 'Supervision of Web host' }))
    expect(group.getByText('Supervised · every 15 min')).toBeInTheDocument()
    expect(
      group.getByText(`Next run in 12 min (${formatTime(minutesFromNow(12))})`),
    ).toBeInTheDocument()
    expect(group.getByText(/^Last run:/)).toHaveTextContent('(3 min ago)')
    expect(group.getByText(/Runs only while DevHQ is open/)).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Pause supervision' }))
    expect(value.setHttpCheckPaused).toHaveBeenCalledWith(web, true)
    expect(screen.getByRole('status')).toHaveTextContent(
      'Supervision paused. No run starts on its own until you resume it.',
    )
  })

  it('shows a paused check without a next run and resumes it', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({
      snapshot,
      monitor: monitor([running(db, { nextRunAt: null })]),
    })
    renderWith(<HttpCheckPanel subject={db} name="Database host" />, value)
    const group = within(screen.getByRole('group', { name: 'Supervision of Database host' }))
    expect(group.getByText('Paused · every 30 min')).toBeInTheDocument()
    expect(group.getByText('No run starts on its own until you resume it.')).toBeInTheDocument()
    expect(group.queryByText(/Next run/)).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Resume supervision' }))
    expect(value.setHttpCheckPaused).toHaveBeenCalledWith(db, false)
    expect(screen.getByRole('status')).toHaveTextContent('Supervision resumed.')
  })

  it('shows a scheduled run in progress and says a pause lets it finish', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({
      snapshot,
      monitor: monitor([running(web, { running: true, nextRunAt: null })]),
    })
    renderWith(<HttpCheckPanel subject={web} name="Web host" />, value)
    expect(screen.getByRole('button', { name: 'Checking…' })).toBeDisabled()
    expect(screen.getByText('Running now')).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Checking for up to 5 s')
    await user.click(screen.getByRole('button', { name: 'Pause supervision' }))
    expect(screen.getByRole('status')).toHaveTextContent(
      'Supervision paused. The run in progress will finish and be recorded; no other run starts on its own.',
    )
  })

  it('shows DevHQ’s last failure as an error, not as evidence', () => {
    renderWith(
      <RealInfrastructurePage />,
      inventoryContext({
        snapshot,
        monitor: monitor([
          running(web, {
            lastError: {
              message: 'Inventory database error: database is locked',
              at: minutesFromNow(-3),
            },
          }),
        ]),
      }),
    )
    const region = within(screen.getByRole('region', { name: 'Web host' }))
    expect(
      region.getByText(
        /DevHQ could not run it at .*: Inventory database error: database is locked/,
      ),
    ).toHaveTextContent('Nothing was recorded; it tries again after the next interval, not before.')
    // Still no evidence at all: the failure was DevHQ's.
    expect(region.getByText('Not observed')).toBeInTheDocument()
    expect(region.queryByText('Unhealthy')).not.toBeInTheDocument()
  })

  it('says an unsupervised check runs only when asked, with no supervision controls', () => {
    renderWith(
      <HttpCheckPanel subject={web} name="Web host" />,
      inventoryContext({
        snapshot: {
          ...snapshot,
          httpChecks: [check(web, { monitor: 'off', monitorIntervalMinutes: null })],
        },
      }),
    )
    expect(screen.getByText(/Not supervised: it runs only when you ask/)).toBeInTheDocument()
    expect(screen.queryByRole('group')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /supervision/ })).not.toBeInTheDocument()
  })
})

describe('choosing a supervision interval', () => {
  it('saves the interval with the check', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot: { ...snapshot, httpChecks: [] } })
    renderWith(<HttpCheckPanel subject={web} name="Web host" />, value)
    await user.click(screen.getByRole('button', { name: 'Configure HTTP check' }))
    await user.type(screen.getByRole('textbox', { name: 'URL' }), 'https://web.example/health')
    const supervise = screen.getByRole('combobox', { name: 'Supervise while DevHQ is open' })
    expect(supervise).toHaveDisplayValue('Off · only when I ask')
    await user.selectOptions(supervise, 'Every 15 min')
    await user.click(screen.getByRole('button', { name: 'Save HTTP check' }))
    expect(value.saveHttpCheck).toHaveBeenCalledWith({
      subject: web,
      url: 'https://web.example/health',
      expectedStatus: 200,
      timeoutSeconds: 5,
      validForMinutes: 60,
      monitorIntervalMinutes: 15,
    })
  })

  it('warns when the result would expire before the next run and shows DevHQ’s refusal', async () => {
    const user = userEvent.setup()
    const refusal =
      'The result must stay valid for longer than the supervision interval (15 min), or the evidence would expire before the next run. Choose a longer validity or a shorter interval.'
    const value = inventoryContext({
      snapshot,
      saveHttpCheck: vi.fn().mockRejectedValue({ code: 'invalid_input', message: refusal }),
    })
    renderWith(<HttpCheckPanel subject={web} name="Web host" />, value)
    await user.click(screen.getByRole('button', { name: 'Edit check' }))
    const supervise = screen.getByRole('combobox', { name: 'Supervise while DevHQ is open' })
    expect(supervise).toHaveDisplayValue('Every 15 min')
    await user.selectOptions(screen.getByRole('combobox', { name: 'Result valid for' }), '15 min')
    expect(supervise).toHaveAccessibleDescription(
      'The result must stay valid for longer than the interval (15 min), or the evidence would expire before the next run.',
    )
    await user.click(screen.getByRole('button', { name: 'Save HTTP check' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(refusal)
    expect(screen.getByRole('form', { name: 'Configure HTTP check' })).toBeInTheDocument()
  })
})

describe('supervision in Overview', () => {
  it('says that nothing supervised is not a sign of health', () => {
    renderWith(
      <RealOverviewPage />,
      inventoryContext({
        snapshot: {
          ...snapshot,
          httpChecks: [check(web, { monitor: 'off', monitorIntervalMinutes: null })],
        },
      }),
    )
    const section = within(screen.getByRole('region', { name: 'Supervision' }))
    expect(section.getByText('Nothing is supervised right now')).toBeInTheDocument()
    expect(section.getByText(/That says nothing about health/)).toBeInTheDocument()
    expect(section.getByText(/Not supervised \(3\)/)).toBeInTheDocument()
    expect(section.queryByRole('list')).not.toBeInTheDocument()
  })

  it('lists what is supervised, what is paused and what is not', () => {
    renderWith(
      <RealOverviewPage />,
      inventoryContext({
        snapshot,
        monitor: monitor([
          running(web),
          running(db, {
            nextRunAt: null,
            lastError: {
              message: 'Inventory database error: disk I/O error',
              at: minutesFromNow(-40),
            },
          }),
        ]),
      }),
    )
    const section = within(screen.getByRole('region', { name: 'Supervision' }))
    expect(section.getByText('1 of 3 systems are supervised')).toBeInTheDocument()
    expect(section.getByText(/Being supervised is not being healthy/)).toBeInTheDocument()
    const [webRow, dbRow] = section.getAllByRole('listitem').map((item) => within(item))
    expect(webRow!.getByRole('link', { name: 'Web host' })).toBeInTheDocument()
    expect(webRow!.getByText('Supervised · every 15 min')).toBeInTheDocument()
    expect(webRow!.getByText(/^Next run in 12 min/)).toBeInTheDocument()
    expect(dbRow!.getByText('Paused · every 30 min')).toBeInTheDocument()
    expect(dbRow!.queryByText(/Next run/)).not.toBeInTheDocument()
    expect(dbRow!.getByText(/DevHQ could not run it/)).toBeInTheDocument()
    // Paused counts as not supervised, next to the system without a check.
    const unwatched = section.getByText(/^Not supervised \(2\)/)
    expect(
      within(unwatched)
        .getAllByRole('link')
        .map((link) => link.textContent),
    ).toEqual(['Shop', 'Database host'])
    expect(section.getByText(/Supervision runs only while DevHQ is open/)).toBeInTheDocument()
    // Supervision is not evidence: nothing was observed, so nothing is current.
    const coverage = within(screen.getByRole('region', { name: 'Evidence coverage' }))
    expect(coverage.getAllByText('Never observed')).toHaveLength(3)
  })
})

function SelectRealInventory() {
  const { setMode } = useInventory()
  return <button onClick={() => setMode('real')}>Select real inventory</button>
}

describe('status pushed by the native process', () => {
  afterEach(() => vi.restoreAllMocks())

  const recordedRun: Observation = {
    id: 'scheduled-1',
    subject: web,
    result: 'unhealthy',
    summary: 'HTTP 503, expected 200, in 12 ms',
    source: 'http',
    observedAt: new Date().toISOString(),
    validUntil: new Date(Date.now() + 60 * 60_000).toISOString(),
    recordedAt: new Date().toISOString(),
    http: {
      url: 'https://web.example/health',
      expectedStatus: 200,
      status: 503,
      latencyMs: 12,
      error: null,
      redirects: [],
    },
    agent: null,
  }

  async function renderLive() {
    let push: (status: MonitorStatus) => void = () => {}
    const watch = vi
      .spyOn(inventoryApi, 'watchHttpMonitor')
      .mockImplementation(async (onStatus) => {
        push = onStatus
      })
    const list = vi.spyOn(inventoryApi, 'list').mockResolvedValue(snapshot)
    const user = userEvent.setup()
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <RealInfrastructurePage />
        </InventoryProvider>
      </MemoryRouter>,
    )
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    const region = within(await screen.findByRole('region', { name: 'Web host' }))
    return { region, list, watch, push: (status: MonitorStatus) => act(() => push(status)) }
  }

  it('shows each scheduled run and the evidence it recorded without reloading the page', async () => {
    const { region, list, watch, push } = await renderLive()
    expect(watch).toHaveBeenCalledTimes(1)
    await push(monitor([running(web, { running: true, nextRunAt: null })]))
    expect(region.getByRole('button', { name: 'Checking…' })).toBeDisabled()
    expect(list).toHaveBeenCalledTimes(1)

    // The run recorded an observation: the snapshot is read again, quietly.
    list.mockResolvedValue({ ...snapshot, observations: [recordedRun] })
    await push(monitor([running(web, { lastRunAt: recordedRun.observedAt })], 1))
    expect(await region.findByText('HTTP 503')).toBeInTheDocument()
    expect(region.getByText('Automated · HTTP check')).toBeInTheDocument()
    expect(region.getByRole('button', { name: 'Run check' })).toBeEnabled()
    expect(list).toHaveBeenCalledTimes(2)
    expect(screen.queryByText('Loading local inventory…')).not.toBeInTheDocument()

    // A status change without new evidence reads nothing.
    await push(monitor([running(web, { nextRunAt: minutesFromNow(20) })], 1))
    expect(list).toHaveBeenCalledTimes(2)
  })

  it('says when supervision status cannot be received', async () => {
    vi.spyOn(inventoryApi, 'watchHttpMonitor').mockRejectedValue({
      code: 'ipc_error',
      message: 'The channel could not be opened',
    })
    vi.spyOn(inventoryApi, 'list').mockResolvedValue(snapshot)
    const user = userEvent.setup()
    render(
      <MemoryRouter>
        <InventoryProvider>
          <SelectRealInventory />
          <RealInfrastructurePage />
        </InventoryProvider>
      </MemoryRouter>,
    )
    await user.click(screen.getByRole('button', { name: 'Select real inventory' }))
    const group = within(await screen.findByRole('group', { name: 'Supervision of Web host' }))
    expect(
      await group.findByText('Supervision status is unavailable: The channel could not be opened'),
    ).toBeInTheDocument()
  })
})
