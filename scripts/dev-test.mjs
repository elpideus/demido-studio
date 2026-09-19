// `pnpm dev-test`: the app on a profile of its own, so trying it never touches
// the profile `pnpm dev` keeps. The profile is `app_local_data_dir`, which
// Tauri derives from the identifier, so a second identifier is a second
// profile: its own settings, runtimes and sessions, and the guided set-up on
// its first launch. `--fresh` deletes it first, which is first launch again.
// `--new-chat` moves the conversation aside instead, keeping everything else:
// this build has one conversation per profile, so an empty log is a new chat.
//
// It also names a workspace, because a window with none offers the model no
// tools at all, and trying the app is mostly trying its tools. The folder is
// inside the test profile, so `--fresh` empties it too. A `DEMIDO_WORKSPACE`
// already set wins.
import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, renameSync, rmSync } from 'node:fs'
import { join } from 'node:path'

if (!process.env.LOCALAPPDATA) {
  console.error('LOCALAPPDATA is not set, so there is nowhere to keep a test profile.')
  process.exit(1)
}

const identifier = 'com.demido.studio.test'
const profile = join(process.env.LOCALAPPDATA, identifier)

if (process.argv.includes('--fresh')) {
  rmSync(profile, { recursive: true, force: true })
  console.log(`Deleted the test profile at ${profile}`)
}

if (process.argv.includes('--new-chat')) {
  const log = join(profile, 'sessions', 'session.jsonl')
  if (existsSync(log)) {
    const aside = `${log}.${new Date().toISOString().replace(/[:.]/g, '-')}`
    renameSync(log, aside)
    console.log(`Moved the last conversation to ${aside}`)
  }
}

const workspace = process.env.DEMIDO_WORKSPACE ?? join(profile, 'workspace')
mkdirSync(workspace, { recursive: true })

console.log(`Test profile: ${profile}`)
console.log(`Workspace:    ${workspace}`)
const child = spawn('pnpm', ['tauri', 'dev', '--config', 'src-tauri/tauri.test.conf.json'], {
  stdio: 'inherit',
  shell: true,
  env: { ...process.env, DEMIDO_WORKSPACE: workspace },
})
child.on('exit', (code) => process.exit(code ?? 0))
