import { useCallback, useEffect, useState } from 'react';
import { CheckCircle2, CircleAlert, Cpu, Download, FolderOpen, Power, ScrollText, ShieldAlert, X } from 'lucide-react';
import { Badge, Button, Dialog, Field, IconButton, Select, Spinner, Switch, TextField, formatBytes } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import type { Settings, SpeechChoice } from '@/lib/types';
import { useApp } from '@/stores/app';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { microphones, unlockNames } from '@/voice/recorder';
import s from './settings.module.css';
import styles from './GeneralTab.module.css';

const TOOL_LABELS: Record<string, string> = { run_python: 'Run Python code', run_command: 'Run commands' };

const APPROVAL_CHOICES: Array<{ value: string; label: string }> = [
  { value: '30', label: '30 s' },
  { value: '60', label: '1 min' },
  { value: '120', label: '2 min' },
  { value: '300', label: '5 min' },
];

/** The fixed choices, plus the saved value when it was set to something else (by hand in settings.json). */
function approvalOptions(current: number): Array<{ value: string; label: string }> {
  if (APPROVAL_CHOICES.some((c) => c.value === String(current))) return APPROVAL_CHOICES;
  return [...APPROVAL_CHOICES, { value: String(current), label: `${current} s` }].sort(
    (a, b) => Number(a.value) - Number(b.value),
  );
}

function RuntimeLogs({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [lines, setLines] = useState<string[]>([]);
  useEffect(() => {
    if (open) void api.runtimeLogs().then(setLines);
  }, [open]);
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Runtime log"
      description="The last lines llama-server printed."
      width={760}
    >
      <pre className={styles.logs}>{lines.length ? lines.join('\n') : 'Nothing logged yet.'}</pre>
    </Dialog>
  );
}

/**
 * When long chats are summarized to fit the model's context window. The threshold goes through
 * `/autocompact`, which reads it however it is written and says what it means for the model.
 */
function AutoCompact({ settings }: { settings: Settings }) {
  const patch = useApp((st) => st.patchSettings);
  const models = useModels((st) => st.models);
  const model = models.find((m) => m.isDefault && m.enabled) ?? models.find((m) => m.enabled);
  const modelId = model?.id ?? '';
  // What the model can reach changes with the window its server took as it loaded.
  const runtime = useModels((st) => st.runtime);
  const served = `${runtime?.state}:${runtime?.modelId}:${runtime?.contextLength}`;
  const saved = settings.autoCompactTokens == null ? '' : settings.autoCompactTokens.toLocaleString();
  // What is being typed; null while the saved value shows.
  const [draft, setDraft] = useState<string | null>(null);
  const [meaning, setMeaning] = useState<string | null>(null);

  const autocompact = useCallback(
    async (args: string) => {
      const outcome = await api.runSlashCommand(null, `/autocompact ${args}`.trim(), modelId, []);
      if (outcome.kind !== 'done') return;
      if (outcome.settings) useApp.setState({ settings: outcome.settings });
      setMeaning(outcome.text);
    },
    [modelId],
  );

  useEffect(() => {
    autocompact('').catch(() => setMeaning(null));
  }, [autocompact, settings.autoCompact, settings.autoCompactTokens, served, model?.settings]);

  const commit = () => {
    if (draft === null) return;
    const text = draft.trim();
    setDraft(null);
    if (text === saved) return;
    autocompact(text || 'auto').catch((e) => toast.error('Could not change when chats are compacted', errorText(e)));
  };

  return (
    <div className={`${s.card} ${s.cardPad} ${s.stack}`}>
      <Field
        layout="inline"
        label="Compact long chats automatically"
        description={
          meaning ?? "Summarizes the earlier conversation when a chat nears the end of the model's context window."
        }
      >
        <Switch
          checked={settings.autoCompact ?? true}
          onChange={(v) => void patch({ autoCompact: v })}
          label="Compact long chats automatically"
        />
      </Field>
      <Field
        layout="inline"
        label="Compact at"
        description="Tokens, such as 12000, 12k or 12.5k. Leave it empty to compact just before the context window is full. A model can have its own, in Settings, Models. Type /compact in a chat to compact it now."
      >
        <TextField
          size="sm"
          className={styles.tokens}
          aria-label="Compact at"
          placeholder="Automatic"
          value={draft ?? saved}
          disabled={settings.autoCompact === false}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur();
            else if (e.key === 'Escape') setDraft(null);
          }}
        />
      </Field>
    </div>
  );
}

/** The microphone, the speech model that writes down what is said, and where the voice goes. */
function Voice({ settings }: { settings: Settings }) {
  const patch = useApp((st) => st.patchSettings);
  const [mics, setMics] = useState<MediaDeviceInfo[]>([]);
  const [speech, setSpeech] = useState<SpeechChoice[]>([]);

  const findMics = useCallback(() => {
    microphones().then(setMics, () => setMics([]));
  }, []);

  useEffect(() => {
    findMics();
    navigator.mediaDevices?.addEventListener?.('devicechange', findMics);
    return () => navigator.mediaDevices?.removeEventListener?.('devicechange', findMics);
  }, [findMics]);

  useEffect(() => {
    const load = () => void api.speechModels().then(setSpeech, () => undefined);
    load();
    // A finished download installs a speech model.
    const unlisten = on('downloads://changed', (job) => {
      if (job.state === 'done') load();
    });
    return () => void unlisten.then((f) => f());
  }, []);

  // Microphones have names only once the app has used one; asking for one names them all.
  const unnamed = mics.some((m) => !m.label);
  const nameMics = () => {
    if (!unnamed) return;
    unlockNames().then(findMics, () => undefined);
  };

  const chosenMic = settings.microphone ?? '';
  const micOptions = [
    { value: '', label: 'Windows default' },
    ...mics.map((m, i) => ({ value: m.deviceId, label: m.label || `Microphone ${i + 1}` })),
  ];
  if (chosenMic && !mics.some((m) => m.deviceId === chosenMic)) {
    micOptions.push({ value: chosenMic, label: 'A microphone not plugged in' });
  }

  const auto = speech.find((m) => m.forThisComputer);
  const chosen = speech.find((m) => m.id === settings.speechModel) ?? auto;
  const speechOptions = [
    { value: '', label: auto ? `Automatic (${auto.name})` : 'Automatic' },
    ...speech.map((m) => ({ value: m.id, label: m.installed ? m.name : `${m.name} · not downloaded` })),
  ];

  const download = () =>
    api.downloadSpeechModel().then(
      (job) => toast.info(`Downloading ${job.name}`, 'Settings, Models shows how far it got.'),
      (e) => toast.error('Could not download the speech model', errorText(e)),
    );

  return (
    <div className={`${s.card} ${s.cardPad} ${s.stack}`}>
      <Field
        layout="inline"
        label="Microphone"
        description="Click the microphone in the message box to talk instead of typing. Esc cancels a recording."
      >
        <Select
          size="sm"
          aria-label="Microphone"
          className={styles.wide}
          value={chosenMic}
          onPointerDown={nameMics}
          onFocus={nameMics}
          onChange={(e) => void patch({ microphone: e.target.value || null })}
          options={micOptions}
        />
      </Field>
      <Field
        layout="inline"
        label="Speech model"
        description={
          chosen && !chosen.installed
            ? `Writes down what you say for models that can’t hear. ${chosen.name} is not downloaded yet (${formatBytes(chosen.size)}).`
            : 'Writes down what you say for models that can’t hear, on this computer. Automatic picks by your graphics memory; 1.7B is more accurate and needs more.'
        }
      >
        <div className={styles.inlineControls}>
          {chosen && !chosen.installed && (
            <Button size="sm" variant="secondary" icon={Download} onClick={() => void download()}>
              Download
            </Button>
          )}
          <Select
            size="sm"
            aria-label="Speech model"
            className={styles.wide}
            value={settings.speechModel ?? ''}
            onChange={(e) => void patch({ speechModel: e.target.value || null })}
            options={speechOptions}
          />
        </div>
      </Field>
      <Field
        layout="inline"
        label="Send my voice to models that can hear"
        description="They get the recording itself, tone and all. Off, every model gets what you said written down instead."
      >
        <Switch
          checked={settings.sendVoice ?? true}
          onChange={(v) => void patch({ sendVoice: v })}
          label="Send my voice to models that can hear"
        />
      </Field>
    </div>
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
  // A backend from before this setting sends none; 60 s is its default.
  const approvalSeconds = settings.downloadApprovalSeconds ?? 60;
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
          <p className={s.subtitle}>This computer, the local AI runtime, conversations, permissions and storage.</p>
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
                  <div className={s.rowMeta}>
                    {c.installed ? (c.version ?? 'Installed') : 'Not installed: run the installer again to add it.'}
                  </div>
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
          <h3 className={s.sectionTitle}>Conversations</h3>
          <AutoCompact settings={settings} />
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Voice</h3>
          <Voice settings={settings} />
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Permissions</h3>
          <div className={s.stack}>
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
                        void api.revokeToolPermission(tool).then(
                          () => useApp.getState().init(),
                          (e) => toast.error('Could not update', errorText(e)),
                        )
                      }
                    />
                  </div>
                ))
              )}
            </div>
            <div className={`${s.card} ${s.cardPad}`}>
              <Field
                layout="inline"
                label="Ask before downloads longer than"
                description="For a longer market data download, the assistant shows the estimate and waits for your OK."
              >
                <Select
                  size="sm"
                  aria-label="Ask before downloads longer than"
                  value={String(approvalSeconds)}
                  onChange={(e) => void patch({ downloadApprovalSeconds: Number(e.target.value) })}
                  options={approvalOptions(approvalSeconds)}
                />
              </Field>
            </div>
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
                <Button
                  size="sm"
                  variant="secondary"
                  icon={FolderOpen}
                  onClick={() => void api.openPath(info.installDir!)}
                >
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
              <div className={s.rowMeta}>
                Free software under the GNU GPL v3 or later.{info.dev ? ' Development build.' : ''}
              </div>
            </div>
            <Button size="sm" variant="ghost" icon={ShieldAlert} onClick={() => setNoticeOpen(true)}>
              Safety notice
            </Button>
          </div>
        </section>
      </div>
      <RuntimeLogs open={logsOpen} onClose={() => setLogsOpen(false)} />
      <Dialog open={noticeOpen} onClose={() => setNoticeOpen(false)} title="Safety notice" width={520}>
        Demido Studio is still under development. AI models can be wrong, misbehave, or be manipulated by content they
        read (prompt injection) into doing things you did not ask for. Review what the assistant does, and only allow
        code you are comfortable with. Nothing here is financial or other professional advice. The software is provided
        as is, without warranty, and its author accepts no responsibility for any consequence of its use.
      </Dialog>
    </div>
  );
}
