import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { Server } from 'lucide-react'

import styles from './App.module.css'

/** What `boot_report` returns. The shape is the Rust struct's, camel-cased. */
type BootReport = {
  version: string
  driven: boolean
}

/**
 * The window this slice opens.
 *
 * It is not the desk. The desk is chat as the surface the app is
 * (`design/shell.md`), and it arrives later in S1. What this draws is the
 * bootstrap surface: proof that the window opens, that its colours and its type
 * come from `design/tokens.css`, and that the IPC channel between the webview
 * and Rust is real rather than merely present.
 */
export function App() {
  const [report, setReport] = useState<BootReport | null>(null)
  const [failure, setFailure] = useState<string | null>(null)

  useEffect(() => {
    invoke<BootReport>('boot_report')
      .then(setReport)
      .catch((error: unknown) => setFailure(String(error)))
  }, [])

  return (
    <main className={styles.rack}>
      <section className={styles.island}>
        <header className={styles.head}>
          <Server className={styles.mark} aria-hidden strokeWidth={1.5} />
          <div>
            <h1 className={styles.title}>Demido Studio</h1>
            <p className={styles.subtitle}>An LLM harness that makes small models behave.</p>
          </div>
        </header>

        <dl className={styles.readout}>
          <div className={styles.row}>
            <dt className={styles.label}>Build</dt>
            <dd className={styles.value}>{__APP_VERSION__}</dd>
          </div>
          <div className={styles.row}>
            <dt className={styles.label}>Backend</dt>
            <dd className={styles.value}>
              {failure ? (
                <span className={styles.bad}>{failure}</span>
              ) : report ? (
                <span className={styles.good}>answering, version {report.version}</span>
              ) : (
                <span className={styles.waiting}>asking</span>
              )}
            </dd>
          </div>
          <div className={styles.row}>
            <dt className={styles.label}>Driving</dt>
            <dd className={styles.value}>
              {report?.driven ? (
                <span className={styles.good}>relaxed, window.__TAURI__ present</span>
              ) : (
                <span className={styles.waiting}>off</span>
              )}
            </dd>
          </div>
        </dl>

        <p className={styles.note}>
          The shell is not built yet. This window exists so that the two gates every later ticket
          closes on already exist and already fail correctly.
        </p>
      </section>
    </main>
  )
}
