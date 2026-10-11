import { describe, expect, it } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import type { Observation, RecordedIncident } from '@/domain/models'
import type { InventorySnapshot } from './inventory-api'
import { InventoryContext } from './inventory-context'
import { RealApplicationDetailPage } from './real-application-detail-page'
import { inventoryContext, minutesFromNow } from './test-context'

function observation(
  resourceId: string,
  result: Observation['result'],
  validUntilMinutes: number,
): Observation {
  return {
    id: `obs-${resourceId}`,
    subject: { type: 'resource', id: resourceId },
    result,
    summary: `${resourceId} checked by hand`,
    source: 'manual',
    observedAt: minutesFromNow(-30),
    validUntil: minutesFromNow(validUntilMinutes),
    recordedAt: minutesFromNow(-30),
    http: null,
    agent: null,
  }
}

const diskFull: RecordedIncident = {
  id: 'inc-disk',
  subject: { type: 'resource', id: 'db-host' },
  title: 'Disk almost full',
  description: '',
  severity: 'warning',
  status: 'open',
  source: 'manual',
  openedAt: minutesFromNow(-45),
  updatedAt: minutesFromNow(-45),
  resolvedAt: null,
  agent: null,
}

const snapshot: InventorySnapshot = {
  applications: [
    {
      id: 'shop',
      name: 'Shop',
      description: '',
      lifecycle: 'production',
      version: null,
      components: [
        { id: 'api', name: 'API', kind: 'backend', resourceId: 'web-host' },
        { id: 'web', name: 'Storefront', kind: 'frontend', resourceId: 'web-host' },
        { id: 'db', name: 'Orders', kind: 'database', resourceId: 'db-host' },
        { id: 'jobs', name: 'Jobs', kind: 'docker', resourceId: null },
      ],
      references: [],
    },
    {
      id: 'blog',
      name: 'Blog',
      description: '',
      lifecycle: 'production',
      version: null,
      components: [{ id: 'blog-web', name: 'Site', kind: 'frontend', resourceId: 'web-host' }],
      references: [],
    },
    {
      id: 'empty',
      name: 'Empty',
      description: '',
      lifecycle: 'development',
      version: null,
      components: [],
      references: [],
    },
  ],
  resources: [
    {
      id: 'web-host',
      name: 'Web host',
      kind: 'server',
      description: '',
      operatingSystem: 'Debian 13',
      references: [],
    },
    {
      id: 'db-host',
      name: 'Database host',
      kind: 'server',
      description: '',
      operatingSystem: '',
      references: [],
    },
    {
      id: 'spare',
      name: 'Spare box',
      kind: 'workstation',
      description: '',
      operatingSystem: '',
      references: [],
    },
  ],
  observations: [observation('web-host', 'healthy', 30), observation('db-host', 'degraded', -5)],
  incidents: [diskFull],
  events: [],
  httpChecks: [
    {
      subject: { type: 'resource', id: 'web-host' },
      url: 'https://web.example/health',
      expectedStatus: 200,
      timeoutSeconds: 5,
      validForMinutes: 60,
      updatedAt: minutesFromNow(-120),
      monitor: 'active',
      monitorIntervalMinutes: 15,
    },
  ],
  changes: [],
  changesSince: null,
  automatedHistory: { limit: 200, truncated: [] },
  sharedRepositories: [],
  localCopies: [],
}

function renderTab(applicationId: string) {
  return render(
    <MemoryRouter initialEntries={[`/applications/${applicationId}?tab=infrastructure`]}>
      <InventoryContext.Provider value={inventoryContext({ snapshot })}>
        <Routes>
          <Route path="/applications/:applicationId" element={<RealApplicationDetailPage />} />
        </Routes>
      </InventoryContext.Provider>
    </MemoryRouter>,
  )
}

describe('an application’s infrastructure', () => {
  it('relates each resource it runs on to its evidence, check, incidents and other users', () => {
    renderTab('shop')
    const panel = within(screen.getByRole('tabpanel'))
    expect(screen.getByRole('tab', { name: 'Infrastructure' })).toHaveAttribute(
      'aria-selected',
      'true',
    )

    const web = within(panel.getByRole('region', { name: 'Web host' }))
    expect(web.getByRole('link', { name: /Web host/ })).toHaveAttribute(
      'href',
      '/infrastructure?resource=web-host',
    )
    expect(web.getByText('Server · Debian 13')).toBeInTheDocument()
    expect(web.getByText('API (backend), Storefront (frontend)')).toBeInTheDocument()
    expect(web.getAllByText('Healthy').length).toBeGreaterThan(0)
    expect(web.getByText('GET https://web.example/health')).toBeInTheDocument()
    expect(web.getByText('Supervised · every 15 min')).toBeInTheDocument()
    expect(web.getByRole('link', { name: /Blog/ })).toHaveAttribute('href', '/applications/blog')
    expect(web.queryByText('Disk almost full')).not.toBeInTheDocument()

    const db = within(panel.getByRole('region', { name: 'Database host' }))
    expect(db.getByText('Orders (database)')).toBeInTheDocument()
    // Expired evidence is never shown as its last result.
    expect(db.getByText('Observation expired')).toBeInTheDocument()
    expect(db.getByText(/Not configured/)).toBeInTheDocument()
    expect(db.getByText('No other application uses it')).toBeInTheDocument()
    expect(db.getByText('Disk almost full')).toBeInTheDocument()

    // Resources this application does not use are not listed.
    expect(panel.queryByRole('region', { name: 'Spare box' })).not.toBeInTheDocument()
    const unlinked = within(panel.getByRole('region', { name: 'Components without a resource' }))
    expect(unlinked.getByText(/Jobs \(docker\)/)).toBeInTheDocument()
  })

  it('explains that an application without components cannot be related yet', () => {
    renderTab('empty')
    const panel = within(screen.getByRole('tabpanel'))
    expect(panel.getByRole('heading', { name: 'No components registered' })).toBeInTheDocument()
    expect(panel.queryByRole('region')).not.toBeInTheDocument()
  })
})
