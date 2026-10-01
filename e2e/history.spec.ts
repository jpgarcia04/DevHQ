import { expect, test } from '@playwright/test'

// The preview's sample inventory: Web host has 260 supervised results, 60 more than a snapshot
// carries, and one result recorded by hand among them.

test('Activity loads older automated results until every one is shown', async ({ page }) => {
  await page.goto('/preview.html#/activity')
  const note = page.getByText(/are kept in the local database but not loaded/)
  await expect(note).toBeVisible()
  await expect(page.getByText(/manual records only/).first()).toBeVisible()

  await page.getByRole('button', { name: 'Show 200 older automated results' }).click()
  await expect(page.getByText('Every automated result of these systems is shown.')).toBeVisible()
  await expect(note).toHaveCount(0)
  await expect(page.getByText(/manual records only/)).toHaveCount(0)
  await expect(page.getByRole('button', { name: /older automated results/ })).toHaveCount(0)
})

test('a resource’s observation history opens from Infrastructure and loads older results', async ({
  page,
}) => {
  await page.goto('/preview.html#/infrastructure?resource=web-host')
  const host = page.getByRole('region', { name: 'Web host', exact: true })
  await expect(host).toBeFocused()

  await host.getByRole('button', { name: 'Show observation history' }).click()
  const history = page.getByRole('region', { name: 'Observation history of Web host' })
  const entries = history.getByRole('list').first().locator(':scope > li')
  await expect(entries).toHaveCount(201)
  await history.getByRole('button', { name: 'Show 200 older automated results' }).click()
  await expect(entries).toHaveCount(261)
  await expect(history.getByText('Every automated result of this system is shown.')).toBeVisible()
  await expect(history.getByRole('button', { name: /older automated results/ })).toHaveCount(0)
})
