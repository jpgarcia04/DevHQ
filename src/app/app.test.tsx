import { describe, expect, it } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { App } from './app'

function renderApp(path = '/') {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <App />
    </MemoryRouter>,
  )
}

describe('workspace navigation', () => {
  it('opens every main area through the sidebar', async () => {
    const user = userEvent.setup()
    renderApp()
    for (const name of [
      'Applications',
      'Infrastructure',
      'Automations',
      'Incidents',
      'Activity',
      'Security',
      'Settings',
      'Overview',
    ]) {
      await user.click(within(screen.getByRole('complementary')).getByRole('link', { name }))
      expect(screen.getByRole('heading', { level: 1, name })).toBeInTheDocument()
    }
  })
  it('keeps the skip link first on launch and moves focus after navigation', async () => {
    const user = userEvent.setup()
    renderApp()
    await user.tab()
    expect(screen.getByRole('link', { name: 'Skip to content' })).toHaveFocus()
    await user.click(
      within(screen.getByRole('complementary')).getByRole('link', { name: 'Activity' }),
    )
    expect(screen.getByRole('main')).toHaveFocus()
  })
  it('opens application cards and navigates every detail tab', async () => {
    const user = userEvent.setup()
    renderApp('/applications')
    await user.click(screen.getByRole('link', { name: 'Open Pass Guardian' }))
    expect(screen.getByRole('heading', { level: 1, name: 'Pass Guardian' })).toBeInTheDocument()
    expect(screen.getByText('3 days ago')).toBeInTheDocument()
    expect(screen.getByText('8 hours ago')).toBeInTheDocument()
    for (const name of [
      'Health',
      'Infrastructure',
      'Activity',
      'Security',
      'Automations',
      'Overview',
    ]) {
      await user.click(screen.getByRole('tab', { name }))
      expect(screen.getByRole('tab', { name })).toHaveAttribute('aria-selected', 'true')
      expect(screen.getByRole('tabpanel')).toBeInTheDocument()
    }
  })
  it('filters applications and recovers from an empty search', async () => {
    const user = userEvent.setup()
    renderApp('/applications')
    await user.selectOptions(screen.getByLabelText('Filter applications'), 'attention')
    expect(screen.getByRole('link', { name: 'Open ClipOps' })).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Open Pass Guardian' })).not.toBeInTheDocument()
    await user.type(screen.getByRole('textbox', { name: 'Search applications' }), 'no-such-app')
    expect(screen.getByText('No applications found')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Clear filters' }))
    expect(screen.getAllByRole('link', { name: /^Open / })).toHaveLength(4)
  })
  it('counts what the demo shows with the noun in the right number', async () => {
    const user = userEvent.setup()
    renderApp('/applications')
    expect(screen.getByText('4 applications')).toBeInTheDocument()
    expect(screen.getByText('4 applications shown')).toBeInTheDocument()
    // Orchestrator's Brain has one open incident; ClipOps, on the same VPS, has two.
    expect(screen.getByText('1 linked incident')).toBeInTheDocument()
    expect(screen.getByText('2 linked incidents')).toBeInTheDocument()
    await user.type(screen.getByRole('textbox', { name: 'Search applications' }), 'ClipOps')
    expect(screen.getByText('1 application shown')).toBeInTheDocument()
  })
  it('filters incidents without changing sample statuses', async () => {
    const user = userEvent.setup()
    renderApp('/incidents')
    await user.selectOptions(screen.getByLabelText('Incident severity'), 'critical')
    expect(screen.getByText('ClipOps deployment failed')).toBeInTheDocument()
    expect(screen.queryByText('Disk usage increased significantly')).not.toBeInTheDocument()
    await user.selectOptions(screen.getByLabelText('Incident status'), 'resolved')
    expect(screen.getByText('No incidents in this view.')).toBeInTheDocument()
  })
  it('supports direct detail links and keyboard tabs', async () => {
    const user = userEvent.setup()
    renderApp('/applications/rentia?tab=health')
    const tab = screen.getByRole('tab', { name: 'Health' })
    expect(tab).toHaveAttribute('aria-selected', 'true')
    tab.focus()
    await user.keyboard('{ArrowRight}')
    expect(screen.getByRole('tab', { name: 'Infrastructure' })).toHaveFocus()
  })
  it('keeps infrastructure-to-application navigation connected', async () => {
    const user = userEvent.setup()
    renderApp('/infrastructure?resource=orchestrator-vps')
    await waitFor(() =>
      expect(screen.getByRole('region', { name: 'Orchestrator VPS' })).toHaveFocus(),
    )
    await user.click(
      within(screen.getByRole('region', { name: 'Orchestrator VPS' })).getByRole('link', {
        name: 'ClipOps',
      }),
    )
    expect(screen.getByRole('heading', { level: 1, name: 'ClipOps' })).toBeInTheDocument()
    expect(screen.getByText('2 linked incidents')).toBeInTheDocument()
  })
  it('connects an application component to its resource and shows its incident', async () => {
    const user = userEvent.setup()
    renderApp('/applications/clipops')
    await user.click(screen.getAllByRole('link', { name: 'Orchestrator VPS' })[0]!)
    const resource = screen.getByRole('region', { name: 'Orchestrator VPS' })
    expect(within(resource).getByText('Disk usage increased significantly')).toBeInTheDocument()
    expect(within(resource).getByText('Open resource incidents')).toBeInTheDocument()
  })
  it('keeps the demo a labelled example that never offers or shows real checks', () => {
    renderApp('/infrastructure')
    expect(screen.getByText('Demo · example data')).toBeInTheDocument()
    expect(screen.getByText('DEMO DATA · NO LIVE CONNECTIONS')).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: /Run check|Configure HTTP check/ }),
    ).not.toBeInTheDocument()
    expect(screen.queryByText('Automated · HTTP check')).not.toBeInTheDocument()
  })
  it.each([
    ['/missing', 'View not found'],
    ['/applications/missing', 'Application not found'],
  ])('handles an unknown path %s', (path, title) => {
    renderApp(path)
    expect(screen.getByRole('heading', { name: title })).toBeInTheDocument()
  })
})
