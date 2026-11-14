import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { devhqVersion } from './version'

// The repository's root, where Vitest runs.
const file = (path: string) => readFileSync(path, 'utf8')

describe("DevHQ's version", () => {
  it('is one number wherever it is written', () => {
    const lock = JSON.parse(file('package-lock.json')) as {
      version: string
      packages: Record<string, { version: string }>
    }
    // Cargo's manifest and lockfile name the package and its version on consecutive lines.
    const cargo = (path: string) => /name = "devhq"\r?\nversion = "([^"]+)"/.exec(file(path))?.[1]
    expect({
      packageJson: (JSON.parse(file('package.json')) as { version: string }).version,
      packageLock: lock.version,
      packageLockRoot: lock.packages['']?.version,
      tauri: (JSON.parse(file('src-tauri/tauri.conf.json')) as { version: string }).version,
      cargoToml: cargo('src-tauri/Cargo.toml'),
      cargoLock: cargo('src-tauri/Cargo.lock'),
    }).toEqual({
      packageJson: devhqVersion,
      packageLock: devhqVersion,
      packageLockRoot: devhqVersion,
      tauri: devhqVersion,
      cargoToml: devhqVersion,
      cargoLock: devhqVersion,
    })
  })

  it('is a plain version: the commit of a build is added by the build, never written here', () => {
    expect(devhqVersion).toMatch(/^\d+\.\d+\.\d+$/)
  })
})
