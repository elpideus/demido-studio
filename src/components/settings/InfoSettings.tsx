import { useEffect, useState } from 'react'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'

type UpdateState = 'idle' | 'checking' | 'available' | 'downloading' | 'up-to-date' | 'error'

const APP_VERSION = __APP_VERSION__

// ponytail: updater throws "the platform `<target>` was not found on the
// response `platforms` object" when latest.json omits the current OS/arch.
// Match on that to hide the update UI; anything else is a real/transient error.
function isUnsupportedPlatform(e: unknown): boolean {
  return /platform .*was not found/i.test(String(e))
}

export function InfoSettings() {
  const [state, setState] = useState<UpdateState>('idle')
  const [newVersion, setNewVersion] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [progress, setProgress] = useState<number>(0)
  // null = still probing, true/false = whether this OS is in latest.json
  const [supported, setSupported] = useState<boolean | null>(null)

  useEffect(() => {
    check()
      .then(() => setSupported(true))
      .catch(e => setSupported(isUnsupportedPlatform(e) ? false : true))
  }, [])

  async function checkForUpdates() {
    setState('checking')
    setError(null)
    try {
      const update = await check()
      if (update?.available) {
        setNewVersion(update.version)
        setState('available')
      } else {
        setState('up-to-date')
      }
    } catch (e) {
      setError(String(e))
      setState('error')
    }
  }

  async function installUpdate() {
    setState('downloading')
    setProgress(0)
    try {
      const update = await check()
      if (!update?.available) return
      let downloaded = 0
      let total = 0
      await update.downloadAndInstall(event => {
        if (event.event === 'Started') {
          total = event.data.contentLength ?? 0
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength
          setProgress(total > 0 ? Math.round((downloaded / total) * 100) : 0)
        }
      })
      await relaunch()
    } catch (e) {
      setError(String(e))
      setState('error')
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-sm font-semibold text-foreground mb-1">About</h3>
        <p className="text-xs text-muted-foreground">Demido Studio</p>
        <p className="text-xs text-muted-foreground">Version {APP_VERSION}</p>
      </div>

      {supported && (
      <div>
        <h3 className="text-sm font-semibold text-foreground mb-3">Updates</h3>

        {state === 'idle' && (
          <button onClick={checkForUpdates} className="px-4 py-2 text-sm bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors">
            Check for Updates
          </button>
        )}

        {state === 'checking' && (
          <p className="text-sm text-muted-foreground">Checking for updates…</p>
        )}

        {state === 'up-to-date' && (
          <div className="space-y-2">
            <p className="text-sm text-muted-foreground">You're up to date.</p>
            <button onClick={checkForUpdates} className="text-xs text-muted-foreground underline">
              Check again
            </button>
          </div>
        )}

        {state === 'available' && (
          <div className="space-y-3">
            <p className="text-sm text-foreground">Update available: <span className="font-semibold">{newVersion}</span></p>
            <button onClick={installUpdate} className="px-4 py-2 text-sm bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors">
              Download & Install
            </button>
          </div>
        )}

        {state === 'downloading' && (
          <div className="space-y-2">
            <p className="text-sm text-muted-foreground">Downloading… {progress > 0 ? `${progress}%` : ''}</p>
            {progress > 0 && (
              <div className="w-full bg-accent rounded-full h-1.5">
                <div className="bg-primary h-1.5 rounded-full transition-all" style={{ width: `${progress}%` }} />
              </div>
            )}
          </div>
        )}

        {state === 'error' && (
          <div className="space-y-2">
            <p className="text-sm text-destructive">Update check failed.</p>
            {error && <p className="text-xs text-muted-foreground font-mono">{error}</p>}
            <button onClick={checkForUpdates} className="text-xs text-muted-foreground underline">
              Try again
            </button>
          </div>
        )}
      </div>
      )}
    </div>
  )
}
