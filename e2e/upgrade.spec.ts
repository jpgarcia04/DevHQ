import { expect, test } from '@playwright/test'

// An inventory in an older schema, as the preview's simulated native process refuses it: the
// window shows what it is, read without changing it, and offers to back it up and upgrade it. The
// preview, like every write, refuses the upgrade, and the window says nothing happened.
test('an outdated inventory is offered a backup and an upgrade, and nothing else', async ({
  page,
}) => {
  await page.goto('/preview.html?scenario=outdated#/')
  const panel = page.getByRole('region', { name: 'Inventory needs a schema upgrade' })
  await expect(panel).toContainText('is a DevHQ inventory in schema version 6')
  await expect(panel).toContainText('this DevHQ reads version 13')
  await expect(panel).toContainText('It holds 69 rows in 7 tables')
  await expect(panel).toContainText('There is no upgrade without this backup.')
  // One way forward, never one without the backup, and nothing to retry.
  await expect(panel.getByRole('button')).toHaveText(['Back up and upgrade'])
  await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0)

  await panel.getByRole('button', { name: 'Back up and upgrade' }).click()
  const alert = panel.getByRole('alert')
  await expect(alert).toContainText('The upgrade did not happen.')
  await expect(alert).toContainText('nothing was saved')
  // Looked at again, it is still what it was: the person can decide again.
  await expect(panel.getByRole('button', { name: 'Back up and upgrade' })).toBeVisible()
  await expect(page.getByRole('region', { name: 'Inventory upgraded' })).toHaveCount(0)
})

// Every other page that reads the inventory shows the same panel, not an empty inventory.
test('every page of the inventory offers the upgrade instead of an empty inventory', async ({
  page,
}) => {
  for (const route of ['#/applications', '#/infrastructure', '#/activity']) {
    await page.goto(`/preview.html?scenario=outdated${route}`)
    await expect(
      page.getByRole('region', { name: 'Inventory needs a schema upgrade' }),
    ).toBeVisible()
    await expect(page.getByText(/Nothing is registered yet/)).toHaveCount(0)
  }
})
