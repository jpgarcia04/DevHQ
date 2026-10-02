import { defineConfig } from '@playwright/test'

// End-to-end tests drive the preview (preview.html): the real window over a simulated native
// process and sample data. No browser is ever downloaded: CI uses the Chrome that GitHub's
// runners already have, and elsewhere DEVHQ_CHROMIUM names a Chromium or Chrome executable.
const executablePath = process.env.DEVHQ_CHROMIUM

export default defineConfig({
  testDir: 'e2e',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  reporter: 'list',
  use: {
    baseURL: 'http://127.0.0.1:1420',
    browserName: 'chromium',
    channel: executablePath ? undefined : 'chrome',
    launchOptions: { executablePath },
    viewport: { width: 1440, height: 940 },
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'npm run dev',
    url: 'http://127.0.0.1:1420/preview.html',
    reuseExistingServer: !process.env.CI,
  },
  // Runs after the server is up: loads the app once, so that no test waits for a cold server.
  globalSetup: './e2e/warm-up.ts',
})
