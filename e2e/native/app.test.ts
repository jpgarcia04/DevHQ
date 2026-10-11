// @vitest-environment node
/**
 * The native walk's first guard: an executable the installer packed keeps the installed DevHQ's
 * data, so the walk must refuse it before it starts any process (the window, tauri-driver or the
 * agent gate whose `status` opens the inventory) and before it prepares or even looks at a data
 * folder. Every executable here is a synthetic file that only carries, or lacks, the bundle
 * marker; nothing is ever run, and the processes and folders the walk would use are stand-ins
 * that record each call.
 */
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('node:child_process', () => ({
  spawn: vi.fn(() => {
    throw new Error('the walk started a process')
  }),
  // The agent gate's answer to `status`, for the one executable allowed this far: the inventory
  // where the walk expects it, which does not exist.
  spawnSync: vi.fn((_binary: string, _args: string[], options: { env: NodeJS.ProcessEnv }) => {
    const data = options.env.XDG_DATA_HOME ?? options.env.APPDATA!
    const database = join(data, 'com.devhq.desktop.dev', 'inventory.sqlite3')
    const status = { database: { database }, exists: false }
    return {
      status: 0,
      stdout: `${JSON.stringify({ id: 1, result: {} })}\n${JSON.stringify({ id: 2, result: { structuredContent: status } })}\n`,
      stderr: '',
    }
  }),
}))
vi.mock('node:fs/promises', async (real) => ({
  ...(await real<typeof import('node:fs/promises')>()),
  mkdtemp: vi.fn(async (prefix: string) => `${prefix}stand-in`),
  rm: vi.fn(),
}))
vi.mock('node:fs', async (real) => {
  const fs = await real<typeof import('node:fs')>()
  return { ...fs, existsSync: vi.fn(fs.existsSync) }
})

const marker = (value: string) => `__TAURI_BUNDLE_TYPE_VAR_${value}`
const folder = mkdtempSync(join(tmpdir(), 'devhq-walk-guard-'))
/** A synthetic executable: filler bytes around the given strings, as a binary holds them. */
function executable(name: string, ...strings: string[]) {
  const path = join(folder, name)
  writeFileSync(path, ['MZ', ...strings, ''].join('\0'.repeat(64)))
  return path
}
// What the bundler leaves: the unpatched marker turned into NSIS's, beside the value
// `bundle_type` compares against.
const packed = executable('packed.exe', marker('NSS'), marker('NSS'))
const unmarked = executable('unmarked.exe', 'no marker at all')
const twice = executable('twice.exe', marker('UNK'), marker('NSS'), marker('UNK'))
const unpacked = executable('unpacked.exe', marker('UNK'), marker('NSS'))

const platform = process.platform
afterEach(() => {
  Object.defineProperty(process, 'platform', { value: platform })
  vi.unstubAllEnvs()
})
afterAll(() => rmSync(folder, { recursive: true, force: true }))

for (const os of ['linux', 'win32'] as const) {
  describe(`on ${os}`, () => {
    /** The walk's helpers as they load on that system, and the stand-ins they reach. */
    async function load() {
      vi.resetModules()
      Object.defineProperty(process, 'platform', { value: os })
      // On Windows the walk looks for its folders under these; synthetic, never the real ones.
      vi.stubEnv('APPDATA', join(folder, os, 'Roaming'))
      vi.stubEnv('LOCALAPPDATA', join(folder, os, 'Local'))
      const app = await import('./app.ts')
      const processes = await import('node:child_process')
      const folders = await import('node:fs/promises')
      const files = await import('node:fs')
      return {
        isolate: app.isolate,
        touched: () => ({
          spawn: vi.mocked(processes.spawn).mock.calls.length,
          spawnSync: vi.mocked(processes.spawnSync).mock.calls.length,
          mkdtemp: vi.mocked(folders.mkdtemp).mock.calls.length,
          existsSync: vi.mocked(files.existsSync).mock.calls.length,
        }),
      }
    }
    const untouched = { spawn: 0, spawnSync: 0, mkdtemp: 0, existsSync: 0 }

    beforeEach(() => vi.clearAllMocks())

    it.each([
      ['packed by the installer', packed, /no unpatched bundle marker/],
      ['with no marker at all', unmarked, /no unpatched bundle marker/],
      ['with the marker twice', twice, /2 unpatched bundle markers/],
    ])('refuses an executable %s before any process or folder', async (_, binary, message) => {
      const { isolate, touched } = await load()
      await expect(isolate(binary)).rejects.toThrow(message)
      expect(touched()).toEqual(untouched)
    })

    // The control: the same stand-ins do see what the walk does once the guard lets it through,
    // so an untouched result above means the guard stopped it, not that nothing was watched.
    it('goes on to its folders and the agent gate with an executable never packed', async () => {
      const { isolate, touched } = await load()
      const isolated = await isolate(unpacked)
      expect(isolated.dataDirectory.endsWith('com.devhq.desktop.dev')).toBe(true)
      const seen = touched()
      expect(seen.spawnSync).toBe(1)
      expect(os === 'win32' ? seen.existsSync : seen.mkdtemp).toBeGreaterThan(0)
    })
  })
}
