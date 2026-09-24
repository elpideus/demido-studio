import { useEffect, useState } from 'react';
import { CheckCircle2, CircleAlert, Cpu, FolderOpen, Power, ScrollText, ShieldAlert, X } from 'lucide-react';
import { Badge, Button, Dialog, Field, IconButton, Spinner, Switch } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { useApp } from '@/stores/app';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import s from './settings.module.css';
import styles from './GeneralTab.module.css';

const TOOL_LABELS: Record<string, string> = { run_python: 'Run Python code' };

function RuntimeLogs({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [lines, setLines] = useState<string[]>([]);
  useEffect(() => {
    if (open) void api.runtimeLogs().then(setLines);
  }, [open]);
  return (
    <Dialog open={open} onClose={onClose} title="Runtime log" description="The last lines llama-server printed." width={760}>
      <pre className={styles.logs}>{lines.length ? lines.join('\n') : 'Nothing logged yet.'}</pre>
    </Dialog>
  );
}

export function GeneralTab() {
  const info = useApp((st) => st.info);
  const settings = useApp((st) => st.settings);
  const patch = useApp((st) => st.patchSettings);
  const runtime = useModels((st) => st.runtime);
  const [logsOpen, setLogsOpen] = useState(false);
  const [noticeOpen, setNoticeOpen] = useState(false);

  if (!info || !settings) return null;
  const runtimeBadge = {
    idle: <Badge>Idle</Badge>,
    loading: <Badge tone="info">Loading</Badge>,
    ready: <Badge tone="accent">Ready</Badge>,
    error: <Badge tone="danger">Error</Badge>,
    unavailable: <Badge tone="warning">Not installed</Badge>,
  }[runtime?.state ?? 'idle'];

  return (
    <div className={s.page}>
      <header className={s.header}>
        <div className={s.headerText}>
          <h2 className={s.title}>General</h2>
          <p className={s.subtitle}>This computer, the local AI runtime, permissions and storage.</p>
        </div>
      </header>
      <div className={s.scroll}>
        <section className={s.section}>
          <h3 className={s.sectionTitle}>This computer</h3>
          <div className={`${s.card} ${s.cardPad}`}>
            <dl className={s.kv}>
              <dt>Graphics</dt>
              <dd>
                {info.gpu ?? 'No dedicated GPU found'}
                {info.vramGb ? ` · ${info.vramGb} GB` : ''}
              </dd>
              <dt>Processor</dt>
              <dd>{info.cpu}</dd>
              <dt>Memory</dt>
              <dd>{info.memoryGb} GB</dd>
              <dt>System</dt>
              <dd>{info.os}</dd>
            </dl>
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>
            AI runtime
            {runtimeBadge}
          </h3>
          <div className={s.rows}>
            <div className={s.row}>
              <Cpu size={18} className={styles.rowIcon} aria-hidden />
              <div className={s.rowMain}>
                <div className={s.rowTitle}>
                  {runtime?.state === 'loading' && <Spinner size={12} />}
                  {runtime?.modelName ?? 'No model loaded'}
                </div>
                <div className={s.rowMeta}>
                  {runtime?.state === 'error'
                    ? runtime.message
                    : runtime?.state === 'ready'
                      ? `${runtime.contextLength?.toLocaleString()} token context${runtime.loadSeconds ? ` · loaded in ${runtime.loadSeconds.toFixed(1)}s` : ''}`
                      : info.backend
                        ? `${info.backend} backend`
                        : 'The local runtime is not installed.'}
                </div>
              </div>
              <div className={s.rowActions}>
                <Button size="sm" variant="ghost" icon={ScrollText} onClick={() => setLogsOpen(true)}>
                  Log
                </Button>
                {runtime?.state === 'ready' && (
                  <Button size="sm" variant="secondary" icon={Power} onClick={() => void api.unloadModel()}>
                    Unload
                  </Button>
                )}
              </div>
            </div>
            {info.components.map((c) => (
              <div key={c.name} className={s.row}>
                {c.installed ? (
                  <CheckCircle2 size={18} className={styles.ok} aria-hidden />
                ) : (
                  <CircleAlert size={18} className={styles.missing} aria-hidden />
                )}
                <div className={s.rowMain}>
                  <div className={s.rowTitle}>{c.name}</div>
                  <div className={s.rowMeta}>{c.installed ? (c.version ?? 'Installed') : 'Not installed: run the installer again to add it.'}</div>
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Startup</h3>
          <div className={`${s.card} ${s.cardPad}`}>
            <Field
              layout="inline"
              label="Load the default model when Demido starts"
              description="The first answer arrives faster. Uses GPU memory while the app is open."
            >
              <Switch
                checked={settings.preloadDefaultModel}
                onChange={(v) => void patch({ preloadDefaultModel: v })}
                label="Preload default model"
              />
            </Field>
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Permissions</h3>
          <div className={s.rows}>
            {settings.alwaysAllowedTools.length === 0 ? (
              <div className={s.row}>
                <div className={s.rowMain}>
                  <div className={s.rowTitle}>The assistant asks before running code</div>
                  <div className={s.rowMeta}>Choosing “Always allow” on a request adds it here.</div>
                </div>
              </div>
            ) : (
              settings.alwaysAllowedTools.map((tool) => (
                <div key={tool} className={s.row}>
                  <div className={s.rowMain}>
                    <div className={s.rowTitle}>{TOOL_LABELS[tool] ?? tool}</div>
                    <div className={s.rowMeta}>Runs without asking</div>
                  </div>
                  <IconButton
                    icon={X}
                    label="Ask again"
                    size="sm"
                    onClick={() =>
                      void api.revokeToolPermission(tool).then(() => useApp.getState().init(), (e) => toast.error('Could not update', errorText(e)))
                    }
                  />
                </div>
              ))
            )}
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Storage</h3>
          <div className={s.rows}>
            <div className={s.row}>
              <div className={s.rowMain}>
                <div className={s.rowTitle}>Your data</div>
                <div className={`${s.rowMeta} ${s.mono}`}>{info.dataDir}</div>
              </div>
              <Button size="sm" variant="secondary" icon={FolderOpen} onClick={() => void api.openPath(info.dataDir)}>
                Open
              </Button>
            </div>
            {info.installDir && (
              <div className={s.row}>
                <div className={s.rowMain}>
                  <div className={s.rowTitle}>Installation</div>
                  <div className={`${s.rowMeta} ${s.mono}`}>{info.installDir}</div>
                </div>
                <Button size="sm" variant="secondary" icon={FolderOpen} onClick={() => void api.openPath(info.installDir!)}>
                  Open
                </Button>
              </div>
            )}
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>About</h3>
          <div className={`${s.card} ${s.cardPad} ${styles.about}`}>
            <div>
              <div className={s.rowTitle}>Demido Studio {info.version}</div>
              <div className={s.rowMeta}>Free software under the GNU GPL v3 or later.{info.dev ? ' Development build.' : ''}</div>
            </div>
            <Button size="sm" variant="ghost" icon={ShieldAlert} onClick={() => setNoticeOpen(true)}>
              Safety notice
            </Button>
          </div>
        </section>
      </div>
      <RuntimeLogs open={logsOpen} onClose={() => setLogsOpen(false)} />
      <Dialog open={noticeOpen} onClose={() => setNoticeOpen(false)} title="Safety notice" width={520}>
        Demido Studio is still under development. AI models can be wrong, misbehave, or be manipulated by content
        they read (prompt injection) into doing things you did not ask for. Review what the assistant does, and only
        allow code you are comfortable with. Nothing here is financial or other professional advice. The software is
        provided as is, without warranty, and its author accepts no responsibility for any consequence of its use.
      </Dialog>
    </div>
  );
}
