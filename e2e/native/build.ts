/**
 * Builds the app the native walk drives: the frontend embedded, without an installer, in debug
 * unless `--release` is given.
 *
 * On Windows this build, and only this one, also opens a DevTools port on 127.0.0.1 in the
 * window's WebView2. msedgedriver can no longer launch a Tauri app itself: since WebView2 150 it
 * hands the app its debugging port and data folder through environment variables, and Tauri
 * sets the browser arguments and data folder through the WebView2 API, which takes precedence,
 * so the port never opens. The walk starts DevHQ and msedgedriver attaches to this port instead.
 * The port comes from a config overlay merged into this build alone; the installer is built
 * from tauri.conf.json as it is, without it.
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { WEBVIEW2_DEBUG_PORT } from './app.ts'

const root = join(import.meta.dirname, '..', '..')
const release = process.argv.includes('--release')
const args = ['build', '--no-bundle', ...(release ? [] : ['--debug'])]

if (process.platform === 'win32') {
  const config = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')) as {
    app: { windows: Record<string, unknown>[] }
  }
  // A merge replaces arrays whole, so the overlay repeats the window with one key added. Setting
  // the arguments replaces wry's defaults, which it keeps, so the WebView behaves as shipped.
  const wryDefaults = '--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection'
  const windows = config.app.windows.map((window, index) =>
    index === 0
      ? {
          ...window,
          additionalBrowserArgs: `${wryDefaults} --remote-debugging-port=${WEBVIEW2_DEBUG_PORT}`,
        }
      : window,
  )
  const folder = join(root, 'test-results')
  mkdirSync(folder, { recursive: true })
  const overlay = join(folder, 'webdriver.conf.json')
  writeFileSync(overlay, JSON.stringify({ app: { windows } }, null, 2))
  args.push('--config', overlay)
}

const cli = join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js')
const result = spawnSync(process.execPath, [cli, ...args], { cwd: root, stdio: 'inherit' })
process.exit(result.status ?? 1)
