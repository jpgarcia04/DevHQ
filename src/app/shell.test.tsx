import { describe, expect, it } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import type { RecordedIncident } from '@/domain/models'
import {
  InventoryContext,
  type InventoryContextValue,
} from '@/features/inventory/inventory-context'
import { inventoryContext, minutesFromNow } from '@/features/inventory/test-context'
import { Shell } from './shell'

const incident: RecordedIncident = {
  id: 'inc',
  subject: { type: 'resource', id: 'host-1' },
  title: 'Disk full',
  description: '',
  severity: 'critical',
  status: 'open',
  source: 'manual',
  openedAt: minutesFromNow(-5),
  updatedAt: minutesFromNow(-5),
  resolvedAt: null,
}

function incidentsLink(value: InventoryContextValue) {
  render(
    <MemoryRouter>
      <InventoryContext.Provider value={value}>
        <Shell />
      </InventoryContext.Provider>
    </MemoryRouter>,
  )
  return within(screen.getByRole('navigation', { name: 'Main navigation' })).getByRole('link', {
    name: 'Incidents',
  })
}

describe('the incident count in the sidebar', () => {
  const base = inventoryContext()
  const snapshot = { ...base.snapshot!, incidents: [incident] }

  it('stays visible while a save reloads the inventory', () => {
    const link = incidentsLink(inventoryContext({ snapshot, loadState: 'loading' }))
    expect(link).toHaveTextContent('Incidents1')
  })

  it('is not shown before the first load or after a failed one', () => {
    expect(
      incidentsLink(inventoryContext({ snapshot: null, loadState: 'loading' })),
    ).toHaveTextContent(/^Incidents$/)
  })
})
