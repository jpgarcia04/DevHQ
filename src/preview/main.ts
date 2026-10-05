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

// Sample data must never pass for a real inventory. The notice is a strip of its own above the
// window rather than floating over it, so it never covers what scrolls beneath; the window,
// which fills the viewport (h-dvh), fills what the strip leaves instead. Inline styles, so
// that no class used only here reaches the production stylesheet.
const strip = 24
const notice = document.createElement('aside')
notice.setAttribute('aria-label', 'Preview')
notice.style.cssText = `box-sizing:border-box;height:${strip}px;display:flex;align-items:center;justify-content:center;padding:0 16px;border-bottom:1px dashed #fcd34d66;background:#101216;color:#fde68a;font:11px system-ui,sans-serif;white-space:nowrap;overflow:hidden;text-overflow:ellipsis`
notice.textContent = `Preview · sample data (${scenario}) · simulated native process · nothing is saved`
document.body.prepend(notice)
const fit = document.createElement('style')
fit.textContent = `#root{height:calc(100dvh - ${strip}px)}#root>div{height:100%}`
document.head.append(fit)

await import('@/main')
