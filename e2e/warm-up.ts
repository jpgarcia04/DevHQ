import { chromium, type FullConfig } from '@playwright/test'

// The dev server transforms each module the first time a page asks for it and keeps none of them
// between runs, so the first page waits for the whole app, Tailwind's stylesheet included. On a
// Windows laptop that took longer than the 5 s a test gives the window to appear, and the first
// test of every worker failed. One page loads the app here, once the server is up and before any
// test starts, with as long as it needs; the tests then find every module ready.
export default async function warmUp(config: FullConfig) {
  const { baseURL, channel, launchOptions } = config.projects[0].use
  const browser = await chromium.launch({ ...launchOptions, channel })
  try {
    const page = await browser.newPage({ baseURL })
    await page.goto('/preview.html#/')
    await page.locator('main h1').first().waitFor({ timeout: 120_000 })
  } finally {
    await browser.close()
  }
}
