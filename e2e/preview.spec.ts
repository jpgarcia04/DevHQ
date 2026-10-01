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
