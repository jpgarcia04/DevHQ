import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type { ReactNode } from 'react'
import type { EntityRef, HttpEvidence, Observation, RecordedEvent } from '@/domain/models'
import { HttpCheckPanel } from './http-check'
import { inventoryApi, type HttpCheck, type InventorySnapshot } from './inventory-api'
import { InventoryContext, useInventory, type InventoryContextValue } from './inventory-context'
import { InventoryProvider } from './inventory-provider'
import { RealApplicationDetailPage } from './real-application-detail-page'
import { RealInfrastructurePage } from './real-infrastructure-page'
import { RealOverviewPage } from './real-overview-page'
import { RealActivityPage } from './records-pages'
import { inventoryContext, minutesFromNow } from './test-context'

const web: EntityRef = { type: 'resource', id: 'web-host' }
const db: EntityRef = { type: 'resource', id: 'db-host' }
const shop: EntityRef = { type: 'application', id: 'shop' }

function evidence(status: number | null, error: string | null = null): HttpEvidence {
  return {
    url: 'http://web.example/health',
    expectedStatus: 200,
    status,
    latencyMs: 42,
    error,
    redirects: [{ status: 301, location: 'https://web.example/health' }],
  }
}

function observation(
  id: string,
  subject: EntityRef,
  result: Observation['result'],
  observedMinutesAgo: number,
  validUntilMinutes: number,
  http: HttpEvidence | null,
): Observation {
  return {
    id,
    subject,
    result,
    summary: http ? `HTTP ${http.status}, expected 200, in 42 ms` : 'Checked the console by hand',
    source: http ? 'http' : 'manual',
    observedAt: minutesFromNow(-observedMinutesAgo),
    validUntil: minutesFromNow(validUntilMinutes),
    recordedAt: minutesFromNow(-observedMinutesAgo),
    http,
    agent: null,
  }
}

function eventFor(item: Observation, title: string): RecordedEvent {
  return {
    id: `evt-${item.id}`,
    subject: item.subject,
    kind: 'observation',
    outcome: item.result === 'healthy' ? 'success' : 'failure',
    title,
    detail: item.summary,
    source: item.source,
    observationId: item.id,
    incidentId: null,
    occurredAt: item.observedAt,
    recordedAt: item.recordedAt,
    agent: null,
  }
}

function check(subject: EntityRef, url = 'https://web.example/health'): HttpCheck {
  return {
    subject,
    url,
    expectedStatus: 200,
    timeoutSeconds: 5,
    validForMinutes: 60,
    updatedAt: minutesFromNow(-60),
    monitor: 'off',
    monitorIntervalMinutes: null,
  }
}

const webFailure = observation('web-500', web, 'unhealthy', 5, 55, evidence(500))
const dbManual = observation('db-manual', db, 'healthy', 10, 50, null)
const shopChecked = observation('shop-http', shop, 'healthy', 2, 58, {
  ...evidence(200),
  url: 'https://shop.example/health',
  redirects: [],
})
const shopManual = observation('shop-manual', shop, 'degraded', 60, 600, null)

const snapshot: InventorySnapshot = {
  applications: [
    {
      id: 'shop',
      name: 'Shop',
      description: '',
      lifecycle: 'production',
      version: null,
      components: [{ id: 'api', name: 'API', kind: 'backend', resourceId: 'db-host' }],
    },
  ],
  resources: [
    { id: 'web-host', name: 'Web host', kind: 'server', description: '', operatingSystem: '' },
    { id: 'db-host', name: 'Database host', kind: 'server', description: '', operatingSystem: '' },
  ],
  observations: [shopChecked, webFailure, dbManual, shopManual],
  incidents: [],
  events: [
    eventFor(shopChecked, 'HTTP check: Healthy'),
    eventFor(webFailure, 'HTTP check: Unhealthy'),
    eventFor(dbManual, 'Observation: Healthy'),
    eventFor(shopManual, 'Observation: Attention required'),
  ],
  // The web check's URL changed after its last run.
  httpChecks: [check(web), check(shop, 'https://shop.example/health')],
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

const automated = 'Automated · HTTP check'
const manual = 'Manual entry'

describe('HTTP check provenance in real views', () => {
  it('distinguishes automated from manual evidence in Overview, with the technical evidence', () => {
    renderWith(<RealOverviewPage />, inventoryContext({ snapshot }))
    const coverage = screen.getByRole('region', { name: 'Evidence coverage' })
    expect(rowFor(coverage, 'Web host').getByText(automated)).toBeInTheDocument()
    expect(rowFor(coverage, 'Shop').getByText(automated)).toBeInTheDocument()
    expect(rowFor(coverage, 'Database host').getByText(manual)).toBeInTheDocument()
    expect(rowFor(coverage, 'Database host').queryByText(automated)).not.toBeInTheDocument()

    const attention = screen.getByRole('region', { name: /Needs attention/ })
    const details = within(within(attention).getByLabelText('HTTP check evidence'))
    expect(details.getByText('GET http://web.example/health')).toBeInTheDocument()
    expect(details.getByText('HTTP 500')).toBeInTheDocument()
    expect(details.getByText(/expected 200/)).toBeInTheDocument()
    expect(details.getByText('42 ms')).toBeInTheDocument()
    expect(details.getByText('301 → https://web.example/health')).toBeInTheDocument()
    expect(within(attention).getByText(automated)).toBeInTheDocument()

    const activity = screen.getByRole('region', { name: 'Recent activity' })
    expect(rowFor(activity, 'Web host').getByText(automated)).toBeInTheDocument()
    expect(rowFor(activity, 'Database host').getByText(manual)).toBeInTheDocument()
  })

  it('shows each resource’s source, evidence and check in Infrastructure', () => {
    renderWith(<RealInfrastructurePage />, inventoryContext({ snapshot }))
    const webHost = within(screen.getByRole('region', { name: 'Web host' }))
    expect(webHost.getByText(automated)).toBeInTheDocument()
    expect(webHost.getByText('HTTP 500')).toBeInTheDocument()
    expect(webHost.getByRole('button', { name: 'Run check' })).toBeEnabled()
    expect(webHost.getByText('GET https://web.example/health')).toBeInTheDocument()
    expect(
      webHost.getByText(
        'The current evidence checked http://web.example/health; the URL has changed since.',
      ),
    ).toBeInTheDocument()
    const database = within(screen.getByRole('region', { name: 'Database host' }))
    expect(database.getByText(manual)).toBeInTheDocument()
    expect(database.queryByLabelText('HTTP check evidence')).not.toBeInTheDocument()
    expect(database.queryByRole('button', { name: 'Run check' })).not.toBeInTheDocument()
    expect(database.getByRole('button', { name: 'Configure HTTP check' })).toBeInTheDocument()
  })

  it('distinguishes sources in application detail, observation history and activity', async () => {
    const user = userEvent.setup()
    renderWith(
      <Routes>
        <Route path="/applications/:applicationId" element={<RealApplicationDetailPage />} />
      </Routes>,
      inventoryContext({ snapshot }),
      '/applications/shop',
    )
    const current = within(screen.getByRole('heading', { name: 'Current evidence' }).parentElement!)
    expect(current.getByText(automated)).toBeInTheDocument()
    expect(
      within(current.getByLabelText('HTTP check evidence')).getByText(
        'GET https://shop.example/health',
      ),
    ).toBeInTheDocument()
    expect(current.queryByText(manual)).not.toBeInTheDocument()
    const context = within(
      screen.getByRole('heading', { name: 'Operational context' }).parentElement!,
    )
    expect(context.getByText(automated)).toBeInTheDocument()

    await user.click(screen.getByRole('tab', { name: 'Health' }))
    const history = within(screen.getByRole('tabpanel')).getAllByRole('listitem')
    expect(within(history[0]!).getByText(automated)).toBeInTheDocument()
    expect(within(history[0]!).getByText('Current')).toBeInTheDocument()
    expect(within(history[1]!).getByText(manual)).toBeInTheDocument()
    expect(within(history[1]!).getByText('Superseded')).toBeInTheDocument()
    expect(within(history[1]!).queryByLabelText('HTTP check evidence')).not.toBeInTheDocument()

    await user.click(screen.getByRole('tab', { name: 'Activity' }))
    const panel = within(screen.getByRole('tabpanel'))
    expect(panel.getByText('HTTP check: Healthy')).toBeInTheDocument()
    expect(panel.getAllByText(automated)).toHaveLength(1)
    // The application's own manual entry and its linked resource's manual entry.
    expect(panel.getAllByText(manual)).toHaveLength(2)
  })

  it('attributes every entry in the Activity timeline to its source', () => {
    renderWith(<RealActivityPage />, inventoryContext({ snapshot }))
    expect(screen.getAllByText(automated)).toHaveLength(2)
    expect(screen.getAllByText(manual)).toHaveLength(2)
    expect(screen.getByText('HTTP check: Unhealthy')).toBeInTheDocument()
  })

  it('shows an expired automated result as expired, exactly like a manual one', () => {
    const expired = observation('web-ok', web, 'healthy', 120, -60, evidence(200))
    renderWith(
      <RealOverviewPage />,
      inventoryContext({ snapshot: { ...snapshot, observations: [expired] } }),
    )
    const coverage = screen.getByRole('region', { name: 'Evidence coverage' })
    expect(rowFor(coverage, 'Web host').getByText('Observation expired')).toBeInTheDocument()
    expect(rowFor(coverage, 'Web host').getByText(automated)).toBeInTheDocument()
    expect(within(coverage).queryByText('Healthy')).not.toBeInTheDocument()
    const attention = screen.getByRole('region', { name: /Needs attention/ })
    expect(within(attention).getByText(/Last result:/)).toHaveTextContent('Last result: Healthy.')
    expect(within(attention).getByText(/^Expired/)).toBeInTheDocument()
  })
})

describe('HTTP check configuration', () => {
  it('saves a check for the subject in context', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot })
    renderWith(<HttpCheckPanel subject={db} name="Database host" />, value)
    await user.click(screen.getByRole('button', { name: 'Configure HTTP check' }))
    await user.type(screen.getByRole('textbox', { name: 'URL' }), 'https://db.example/ready')
    await user.clear(screen.getByRole('spinbutton', { name: 'Expected status' }))
    await user.type(screen.getByRole('spinbutton', { name: 'Expected status' }), '204')
    await user.selectOptions(screen.getByRole('combobox', { name: 'Give up after' }), '2 s')
    await user.selectOptions(screen.getByRole('combobox', { name: 'Result valid for' }), '15 min')
    await user.click(screen.getByRole('button', { name: 'Save HTTP check' }))
    expect(value.saveHttpCheck).toHaveBeenCalledWith({
      subject: db,
      url: 'https://db.example/ready',
      expectedStatus: 204,
      timeoutSeconds: 2,
      validForMinutes: 15,
      monitorIntervalMinutes: null,
    })
    expect(screen.queryByRole('form', { name: 'Configure HTTP check' })).not.toBeInTheDocument()
  })

  it('keeps the form open and shows a configuration DevHQ rejected', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({
      snapshot,
      saveHttpCheck: vi.fn().mockRejectedValue({
        code: 'invalid_input',
        message: 'Credentials in the URL are not supported. DevHQ sends no credentials.',
      }),
    })
    renderWith(<HttpCheckPanel subject={db} name="Database host" />, value)
    await user.click(screen.getByRole('button', { name: 'Configure HTTP check' }))
    await user.type(screen.getByRole('textbox', { name: 'URL' }), 'https://user:pw@db.example/')
    await user.click(screen.getByRole('button', { name: 'Save HTTP check' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Credentials in the URL')
    expect(screen.getByRole('form', { name: 'Configure HTTP check' })).toBeInTheDocument()
  })

  it('removes a check and says its evidence is kept', async () => {
    const user = userEvent.setup()
    const value = inventoryContext({ snapshot })
    renderWith(<HttpCheckPanel subject={web} name="Web host" />, value)
    await user.click(screen.getByRole('button', { name: 'Remove check' }))
    expect(value.removeHttpCheck).toHaveBeenCalledWith(web)
    expect(screen.getByRole('status')).toHaveTextContent(
      'HTTP check removed, with its supervision. Any run in progress was cancelled and recorded nothing. The observations it recorded before are kept.',
    )
  })
})

function SelectRealInventory() {
  const { setMode } = useInventory()
  return <button onClick={() => setMode('real')}>Select real inventory</button>
}

describe('running an HTTP check', () => {
  afterEach(() => vi.restoreAllMocks())

  const configured: InventorySnapshot = {
    ...snapshot,
    observations: [],
    events: [],
    httpChecks: [check(db, 'http://127.0.0.1:9/ready')],
  }
  const refused = {
    ...observation('db-refused', db, 'unhealthy', 0, 60, {
      ...evidence(null, 'Connection refused: the target refused it'),
      url: 'http://127.0.0.1:9/ready',
      redirects: [],
    }),
    summary: 'Connection refused after 12 ms',
  }

  async function renderRealInfrastructure() {
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
    const region = within(await screen.findByRole('region', { name: 'Database host' }))
    return { user, region }
  }

  it('keeps the window usable while it runs and then shows the recorded evidence', async () => {
    const list = vi.spyOn(inventoryApi, 'list').mockResolvedValue(configured)
    let finish: (value: Observation) => void = () => {}
    const run = vi
      .spyOn(inventoryApi, 'runHttpCheck')
      .mockImplementation(() => new Promise((resolve) => (finish = resolve)))
    const { user, region } = await renderRealInfrastructure()
    await user.click(region.getByRole('button', { name: 'Run check' }))
    expect(run).toHaveBeenCalledWith(db)
    expect(region.getByRole('button', { name: 'Checking…' })).toBeDisabled()
    expect(region.getByRole('status')).toHaveTextContent('Checking for up to 5 s')
    // Other work continues: the resource can still be edited meanwhile.
    await user.click(region.getByRole('button', { name: 'Edit' }))
    expect(region.getByRole('heading', { name: 'Edit resource' })).toBeInTheDocument()

    list.mockResolvedValue({ ...configured, observations: [refused] })
    finish(refused)
    expect(await region.findByText(/^Recorded as Unhealthy/)).toHaveTextContent(
      'Recorded as Unhealthy: Connection refused after 12 ms.',
    )
    expect(list).toHaveBeenCalledTimes(2)
    expect(region.getByText(automated)).toBeInTheDocument()
    expect(region.getByText('Connection refused: the target refused it')).toBeInTheDocument()
    expect(region.getByRole('button', { name: 'Run check' })).toBeEnabled()
  })

  it('drops what the last run said once the check is saved with another URL', async () => {
    const list = vi.spyOn(inventoryApi, 'list').mockResolvedValue(configured)
    vi.spyOn(inventoryApi, 'runHttpCheck').mockResolvedValue(refused)
    const edited = check(db, 'https://db.example/ready')
    const save = vi.spyOn(inventoryApi, 'saveHttpCheck').mockResolvedValue(edited)
    const { user, region } = await renderRealInfrastructure()
    list.mockResolvedValue({ ...configured, observations: [refused] })
    await user.click(region.getByRole('button', { name: 'Run check' }))
    expect(await region.findByText(/^Recorded as Unhealthy/)).toBeInTheDocument()

    list.mockResolvedValue({ ...configured, httpChecks: [edited], observations: [refused] })
    await user.click(region.getByRole('button', { name: 'Edit check' }))
    const url = region.getByRole('textbox', { name: 'URL' })
    await user.clear(url)
    await user.type(url, 'https://db.example/ready')
    await user.click(region.getByRole('button', { name: 'Save HTTP check' }))
    expect(save).toHaveBeenCalledOnce()
    expect(await region.findByText('GET https://db.example/ready')).toBeInTheDocument()
    // That run checked the old URL; the evidence still says so, the button row no longer does.
    expect(region.queryByText(/^Recorded as/)).not.toBeInTheDocument()
    expect(
      region.getByText(
        'The current evidence checked http://127.0.0.1:9/ready; the URL has changed since.',
      ),
    ).toBeInTheDocument()
  })

  it('shows a DevHQ failure as a recoverable error and records nothing', async () => {
    const list = vi.spyOn(inventoryApi, 'list').mockResolvedValue(configured)
    const run = vi
      .spyOn(inventoryApi, 'runHttpCheck')
      .mockRejectedValueOnce({
        code: 'database_error',
        message: 'Inventory database error: disk I/O error',
      })
      .mockResolvedValueOnce(refused)
    const { user, region } = await renderRealInfrastructure()
    await user.click(region.getByRole('button', { name: 'Run check' }))
    expect(await region.findByRole('alert')).toHaveTextContent(
      'Inventory database error: disk I/O error. No observation was recorded.',
    )
    // Not evidence: the resource is still unobserved and nothing was reloaded.
    expect(region.getByText('Not observed')).toBeInTheDocument()
    expect(region.queryByText(automated)).not.toBeInTheDocument()
    expect(list).toHaveBeenCalledTimes(1)

    list.mockResolvedValue({ ...configured, observations: [refused] })
    await user.click(region.getByRole('button', { name: 'Run check' }))
    expect(await region.findByText(automated)).toBeInTheDocument()
    expect(region.queryByRole('alert')).not.toBeInTheDocument()
    expect(run).toHaveBeenCalledTimes(2)
  })
})
