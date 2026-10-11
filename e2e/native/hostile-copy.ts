/**
 * A working copy that names a marker program wherever Git would run one when it reads a copy:
 * fsmonitor, filter drivers, an external diff, a credential helper, an SSH command, a signing
 * program and hooks. The marker only appends a line to a file outside the copy, so the walk can
 * tell whether anything ran. The Rust battery (`local_copies/battery.rs`) covers every key Git
 * knows; this copy is what the walk reads through the real window, the real confined reader and
 * the real gate, and `git status` on it at the end shows that the config is live.
 *
 * It holds one commit on no remote branch (its only remote was never fetched), one changed file,
 * one new file and one file changed under a filter driver DevHQ does not run.
 */
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { join, relative } from 'node:path'

/** The installed Git, without the machine's own config: the copy's config is all it reads. */
function git(dir: string, args: string[], env: NodeJS.ProcessEnv = {}) {
  const empty = join(dir, '..', 'empty-gitconfig')
  if (!existsSync(empty)) writeFileSync(empty, '')
  return execFileSync('git', args, {
    cwd: dir,
    env: {
      ...process.env,
      GIT_CONFIG_NOSYSTEM: '1',
      GIT_CONFIG_GLOBAL: empty,
      GIT_AUTHOR_NAME: 'Walk',
      GIT_AUTHOR_EMAIL: 'walk@example.invalid',
      GIT_COMMITTER_NAME: 'Walk',
      GIT_COMMITTER_EMAIL: 'walk@example.invalid',
      ...env,
    },
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
}

/** Every file and folder under `dir`, with its size, modification time and content hash. */
function snapshot(dir: string) {
  const entries = new Map<string, string>()
  const walk = (folder: string) => {
    for (const name of readdirSync(folder)) {
      const path = join(folder, name)
      const stat = statSync(path)
      const key = relative(dir, path)
      if (stat.isDirectory()) {
        entries.set(`${key}/`, `${stat.mtimeMs}`)
        walk(path)
      } else {
        const hash = createHash('sha256').update(readFileSync(path)).digest('hex')
        entries.set(key, `${stat.size} ${stat.mtimeMs} ${hash}`)
      }
    }
  }
  walk(dir)
  return entries
}

/** What differs between two snapshots: files changed, created or gone. */
function differences(before: Map<string, string>, after: Map<string, string>) {
  const changed: string[] = []
  for (const [key, value] of after)
    if (!before.has(key)) changed.push(`new ${key}`)
    else if (before.get(key) !== value) changed.push(`changed ${key}`)
  for (const key of before.keys()) if (!after.has(key)) changed.push(`gone ${key}`)
  return changed
}

export function hostileCopy(folder: string, remote: string) {
  mkdirSync(folder, { recursive: true })
  const copy = join(folder, 'status-page')
  const marks = join(folder, 'marks.txt')
  // Git for Windows runs these through its own sh, which takes C:/ paths.
  const slash = (path: string) => path.replaceAll('\\', '/')
  const marker = join(folder, 'marker.sh')
  writeFileSync(marker, `#!/bin/sh\necho "$*" >> '${slash(marks)}'\n`)
  chmodSync(marker, 0o755)

  mkdirSync(copy)
  git(copy, ['init', '-q', '-b', 'main'])
  writeFileSync(join(copy, 'a.txt'), 'one\n')
  writeFileSync(join(copy, 'data.bin'), 'raw\n')
  writeFileSync(join(copy, '.gitattributes'), '*.bin filter=hostile\n')
  git(copy, ['add', '-A'])
  git(copy, ['commit', '-q', '-m', 'first'])
  git(copy, ['remote', 'add', 'origin', remote])
  writeFileSync(join(copy, 'a.txt'), 'one\ntwo\n')
  writeFileSync(join(copy, 'data.bin'), 'raw, changed\n')
  writeFileSync(join(copy, 'b.txt'), 'new\n')

  // Last, so that the Git building the copy never runs any of it.
  const run = (label: string) => `'${slash(marker)}' ${label}`
  for (const [key, value] of [
    ['core.fsmonitor', run('fsmonitor')],
    ['filter.hostile.clean', run('clean')],
    ['filter.hostile.smudge', run('smudge')],
    ['diff.external', run('diff')],
    ['credential.helper', `!${run('credential')}`],
    ['core.sshCommand', run('ssh')],
    ['gpg.program', run('gpg')],
    ['log.showSignature', 'true'],
  ])
    git(copy, ['config', key!, value!])
  const hooks = join(copy, '.git', 'hooks')
  // Git's templates usually make it; a Git without them does not.
  mkdirSync(hooks, { recursive: true })
  for (const hook of [
    'post-index-change',
    'reference-transaction',
    'post-checkout',
    'pre-commit',
  ]) {
    writeFileSync(join(hooks, hook), `#!/bin/sh\n'${slash(marker)}' hook-${hook}\n`)
    chmodSync(join(hooks, hook), 0o755)
  }

  const before = snapshot(copy)
  const marked = () => (existsSync(marks) ? readFileSync(marks, 'utf8').trim() : '')
  return {
    path: copy,
    /** Fails if any marker ran or any file of the copy changed, appeared or went. */
    assertUntouched(what: string) {
      assert.equal(marked(), '', `${what} ran programs the copy names`)
      assert.deepEqual(differences(before, snapshot(copy)), [], `${what} wrote to the copy`)
    },
    /** The installed Git reading the same copy: the marks it leaves show the config is live. */
    gitStatusMarks() {
      git(copy, ['status', '--porcelain'])
      return marked()
    },
  }
}
