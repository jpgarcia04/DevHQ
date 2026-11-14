import { expect, test } from '@playwright/test'

// The preview says its data is a sample, and must say it without hiding any of the window it
// labels: at a width where pages span the whole window, nothing may scroll beneath it.
test('the preview notice covers no part of the window', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 700 })
  await page.goto('/preview.html#/activity')
  const notice = page.getByRole('complementary', { name: 'Preview' })
  await expect(notice).toHaveText(/sample data \(history\).*nothing is saved/)
  // The window is the shell React renders into #root, from the sidebar to the footer.
  const window = page.locator('#root > div')
  await expect(page.getByRole('contentinfo')).toBeInViewport({ ratio: 1 })
  const strip = (await notice.boundingBox())!
  const shell = (await window.boundingBox())!
  expect(strip.y + strip.height).toBeLessThanOrEqual(shell.y)
  expect(shell.y + shell.height).toBeLessThanOrEqual(700)
})

// Installations are read as data, when asked, and what a reading found is said in the past: the
// version the file declared against the declared one, between which readings the build changed,
// and how many processes were running from it. The preview refuses to read, like every write.
test('an installation shows what its file declared, beside what the inventory declares', async ({
  page,
}) => {
  await page.goto('/preview.html#/')
  const panel = page.locator('section[aria-labelledby="installations-heading"]')
  await expect(panel.getByText('2 installations on this computer,')).toBeVisible()
  await expect(panel.getByText('Declared version is not the installed one (1)')).toBeVisible()
  await expect(
    panel.getByText('The inventory declares 2.4.1; the file declared 2.5.0+7c1e9d2.'),
  ).toBeVisible()
  await expect(panel.getByText('Not found where declared (1)')).toBeVisible()
  // Beside Needs attention, never in it.
  await expect(panel.getByText(/Not health: none of it is in Needs attention/)).toBeVisible()
  await panel.getByRole('button', { name: 'Read all installations on this computer' }).click()
  await expect(panel.getByRole('alert')).toContainText('Nothing was read.')

  await page.goto('/preview.html#/applications/shop')
  const reference = page
    .getByRole('list', { name: 'References' })
    .locator(':scope > li')
    .filter({ hasText: 'shop.exe' })
  await expect(reference.getByText('Automated · Local read · asked by claude-code')).toBeVisible()
  const facts = reference.getByLabel('What the reading found')
  await expect(facts.getByText('2.5.0+7c1e9d2')).toBeVisible()
  await expect(facts.getByText(/^2 processes were running from it/)).toBeVisible()
  await expect(reference.getByText(/One of the two is out of date\.$/)).toBeVisible()
  await expect(reference.getByText(/it changed between those two readings/)).toBeVisible()
  // Every reading of its installations loads only when asked, newest first.
  await page.getByRole('button', { name: 'Show every installation reading' }).click()
  const history = page.getByRole('region', { name: 'Installation readings of Shop' })
  await expect(history.getByRole('listitem')).toHaveCount(3)
  await expect(history.getByText('All 3 readings of its installations.')).toBeVisible()
})
