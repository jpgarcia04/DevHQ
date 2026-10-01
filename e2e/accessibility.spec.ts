import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

// Every page of Mi inventario over the preview's sample data, a few states that only appear
// after an action, and the demo, audited with axe-core's default rules (WCAG A and AA, and best
// practices such as landmarks and heading order).
const pages: { name: string; path: string; open?: string }[] = [
  { name: 'overview', path: '/preview.html#/' },
  { name: 'overview, nothing registered', path: '/preview.html?scenario=empty#/' },
  { name: 'overview, inventory unavailable', path: '/preview.html?scenario=unavailable#/' },
  { name: 'applications', path: '/preview.html#/applications' },
  { name: 'new application form', path: '/preview.html#/applications', open: 'New application' },
  { name: 'application', path: '/preview.html#/applications/shop' },
  { name: 'application health', path: '/preview.html#/applications/shop?tab=health' },
  {
    name: 'application infrastructure',
    path: '/preview.html#/applications/shop?tab=infrastructure',
  },
  { name: 'application activity', path: '/preview.html#/applications/shop?tab=activity' },
  { name: 'application security', path: '/preview.html#/applications/shop?tab=security' },
  {
    name: 'record observation form',
    path: '/preview.html#/applications/shop',
    open: 'Record observation',
  },
  { name: 'HTTP check form', path: '/preview.html#/applications/shop', open: 'Edit check' },
  { name: 'infrastructure', path: '/preview.html#/infrastructure' },
  {
    name: 'resource history',
    path: '/preview.html#/infrastructure?resource=web-host',
    open: 'Show observation history',
  },
  { name: 'incidents', path: '/preview.html#/incidents?incident=inc-disk' },
  { name: 'activity', path: '/preview.html#/activity' },
  { name: 'settings', path: '/preview.html#/settings' },
  { name: 'unknown view', path: '/preview.html#/nowhere' },
  { name: 'demo overview', path: '/#/' },
  { name: 'demo applications', path: '/#/applications' },
  { name: 'demo application', path: '/#/applications/clipops?tab=infrastructure' },
  { name: 'demo incidents', path: '/#/incidents' },
  { name: 'demo activity', path: '/#/activity' },
  { name: 'demo automations', path: '/#/automations' },
  { name: 'demo security', path: '/#/security' },
]

for (const { name, path, open } of pages) {
  test(`${name} has no accessibility violations`, async ({ page }) => {
    await page.goto(path)
    await expect(page.locator('main h1, main [role="status"]').first()).toBeVisible()
    if (open) await page.getByRole('button', { name: open }).first().click()
    const { violations } = await new AxeBuilder({ page })
      // The demo cards' artwork repeats the lifecycle in faint lettering: pure decoration,
      // hidden from assistive technology and exempt from contrast (WCAG 1.4.3).
      .exclude('.app-art [aria-hidden="true"]')
      .analyze()
    expect(
      violations.map(({ id, help, nodes }) => ({
        id,
        help,
        nodes: nodes.map((node) => node.target.join(' ')),
      })),
    ).toEqual([])
  })
}
