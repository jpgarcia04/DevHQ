// The window with a simulated native process, for development: Mi inventario over sample data,
// without Rust, SQLite or a network. Only preview.html loads this module, so none of it is in
// the production bundle, which is built from index.html.
import { mockIPC } from '@tauri-apps/api/mocks'
import { previewBackend } from './backend'
import { sampleDatabase, scenarios, type Scenario } from './sample'

const requested = new URLSearchParams(window.location.search).get('scenario')
const scenario: Scenario = scenarios.find((item) => item === requested) ?? 'history'

// Before the app loads: it decides whether a native process is there when it is imported.
const backend = previewBackend(sampleDatabase(scenario, Date.now()))
mockIPC((command, args) => backend(command, args as Record<string, unknown> | undefined))

// Sample data must never pass for a real inventory.
const notice = document.createElement('aside')
notice.setAttribute('aria-label', 'Preview')
// Inline, so that no class used only here reaches the production stylesheet.
notice.style.cssText =
  'position:fixed;bottom:36px;right:16px;z-index:50;padding:6px 12px;border:1px dashed #fcd34d66;border-radius:6px;background:#101216f2;color:#fde68a;font:11px system-ui,sans-serif'
notice.textContent = `Preview · sample data (${scenario}) · simulated native process · nothing is saved`
document.body.append(notice)

await import('@/main')
