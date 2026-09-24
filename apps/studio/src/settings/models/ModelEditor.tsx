import { useEffect, useMemo, useState } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { ArrowLeft, ImageMinus, ImagePlus, RotateCcw, Save } from 'lucide-react';
import { Avatar, Badge, Button, Field, Select, Slider, Switch, TextArea, TextField, formatBytes } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { fileUrl } from '@/lib/format';
import type { ModelEntry, ModelSettings } from '@/lib/types';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import s from '../settings.module.css';
import styles from './Models.module.css';

type NumKey = 'temperature' | 'topP' | 'topK' | 'minP' | 'repeatPenalty';

const PARAMS: Array<{ key: NumKey; label: string; hint: string; min: number; max: number; step: number; local?: boolean }> = [
  { key: 'temperature', label: 'Temperature', hint: 'Higher is more creative, lower more focused', min: 0, max: 2, step: 0.05 },
  { key: 'topP', label: 'Top P', hint: 'Keeps the most likely words that add up to this share', min: 0, max: 1, step: 0.01 },
  { key: 'topK', label: 'Top K', hint: 'Considers only this many candidate words', min: 0, max: 200, step: 1 },
  { key: 'minP', label: 'Min P', hint: 'Drops words far less likely than the best one', min: 0, max: 1, step: 0.01, local: true },
  { key: 'repeatPenalty', label: 'Repeat penalty', hint: 'Discourages repeating the same words', min: 1, max: 2, step: 0.01, local: true },
];

const CONTEXTS = [4096, 8192, 16384, 24576, 32768, 49152, 65536, 98304, 131072, 196608, 262144];

function ParamRow({
  label,
  hint,
  value,
  fallback,
  min,
  max,
  step,
  onChange,
}: {
  label: string;
  hint: string;
  value: number | null | undefined;
  fallback: number | null;
  min: number;
  max: number;
  step: number;
  onChange: (v: number | null) => void;
}) {
  const shown = value ?? fallback ?? min;
  return (
    <div className={styles.paramRow}>
      <div className={styles.paramLabel}>
        <span className={styles.paramName}>{label}</span>
        <span className={styles.paramHint}>{hint}</span>
      </div>
      <Slider
        label={label}
        value={shown}
        min={min}
        max={max}
        step={step}
        onChange={onChange}
        format={(v) => (step >= 1 ? String(Math.round(v)) : v.toFixed(2))}
      />
      {value !== null && value !== undefined ? (
        <button type="button" className={styles.reset} onClick={() => onChange(null)}>
          Use default
        </button>
      ) : (
        <span className={styles.paramHint} style={{ justifySelf: 'end' }}>
          Default
        </span>
      )}
    </div>
  );
}

export function ModelEditor({ id, onBack }: { id: string; onBack: () => void }) {
  const model = useModels((st) => st.models.find((m) => m.id === id));
  const [draft, setDraft] = useState<ModelSettings>(model?.settings ?? {});
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (model) setDraft(model.settings);
    // Reset only when switching models, not on every refresh of the list.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  const dirty = useMemo(() => JSON.stringify(normalize(draft)) !== JSON.stringify(normalize(model?.settings ?? {})), [draft, model]);

  if (!model) {
    return (
      <div className={s.page}>
        <header className={s.header}>
          <button type="button" className={s.back} onClick={onBack}>
            <ArrowLeft size={14} /> Models
          </button>
        </header>
        <div className={s.scroll}>This model is no longer available.</div>
      </div>
    );
  }

  const local = model.source === 'local';
  const set = (patch: Partial<ModelSettings>) => setDraft((d) => ({ ...d, ...patch }));
  const save = async (settings: ModelSettings) => {
    setSaving(true);
    try {
      const updated = await api.updateModel(model.id, settings);
      setDraft(updated.settings);
      toast.success('Saved', `${updated.name} uses the new settings from its next answer.`);
    } catch (e) {
      toast.error('Could not save', errorText(e));
    } finally {
      setSaving(false);
    }
  };
  const pickAvatar = async () => {
    const file = await openDialog({
      title: 'Choose a picture',
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'svg'] }],
    });
    if (typeof file !== 'string') return;
    try {
      const updated = await api.importModelAvatar(model.id, file);
      setDraft((d) => ({ ...d, avatar: updated.settings.avatar }));
    } catch (e) {
      toast.error('Could not use that picture', errorText(e));
    }
  };
  const maxContext = model.maxContext ?? 262144;
  const contextOptions = CONTEXTS.filter((c) => c <= Math.max(maxContext, 4096));

  return (
    <div className={s.page}>
      <header className={s.header}>
        <div className={s.headerText}>
          <button type="button" className={s.back} onClick={onBack}>
            <ArrowLeft size={14} aria-hidden /> Models
          </button>
          <div className={styles.editorHead}>
            <button type="button" className={styles.avatarButton} onClick={() => void pickAvatar()} aria-label="Change picture">
              <Avatar name={draft.name || model.defaultName} src={fileUrl(model.avatarPath)} size={52} />
            </button>
            <div>
              <h2 className={s.title}>{draft.name || model.defaultName}</h2>
              <p className={s.subtitle}>
                {local ? [model.parameters, model.quant, model.size ? formatBytes(model.size) : null].filter(Boolean).join(' · ') : model.providerName}
                {model.isDefault && <> · <Badge tone="accent">Default</Badge></>}
              </p>
            </div>
          </div>
        </div>
        <div className={s.headerActions}>
          <Switch checked={draft.enabled ?? model.enabled} onChange={(v) => set({ enabled: v })} label="Enabled" />
        </div>
      </header>

      <div className={s.scroll}>
        <section className={s.section}>
          <h3 className={s.sectionTitle}>Identity</h3>
          <div className={`${s.card} ${s.cardPad} ${s.stack}`}>
            <div className={s.grid2}>
              <Field label="Name">
                <TextField value={draft.name ?? ''} placeholder={model.defaultName} onChange={(e) => set({ name: e.target.value || null })} />
              </Field>
              <Field label="Picture">
                <div style={{ display: 'flex', gap: 8 }}>
                  <Button size="md" variant="secondary" icon={ImagePlus} onClick={() => void pickAvatar()}>
                    Choose
                  </Button>
                  {draft.avatar && (
                    <Button size="md" variant="ghost" icon={ImageMinus} onClick={() => set({ avatar: null })}>
                      Remove
                    </Button>
                  )}
                </div>
              </Field>
            </div>
            <Field label="Description" description="A note for yourself, shown nowhere else.">
              <TextField value={draft.description ?? ''} placeholder="Optional" onChange={(e) => set({ description: e.target.value || null })} />
            </Field>
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Instructions</h3>
          <div className={`${s.card} ${s.cardPad}`}>
            <Field label="System prompt" description="Added to every conversation with this model: its role, tone, or rules to follow.">
              <TextArea
                autoSize={{ min: 4, max: 16 }}
                value={draft.systemPrompt ?? ''}
                placeholder="For example: You are a concise financial analyst. Answer with bullet points."
                onChange={(e) => set({ systemPrompt: e.target.value || null })}
              />
            </Field>
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Sampling</h3>
          <div className={`${s.card} ${s.cardPad} ${s.stack}`}>
            {PARAMS.filter((p) => local || !p.local).map((p) => (
              <ParamRow
                key={p.key}
                label={p.label}
                hint={p.hint}
                value={draft[p.key]}
                fallback={model.effective[p.key]}
                min={p.min}
                max={p.max}
                step={p.step}
                onChange={(v) => set({ [p.key]: v } as Partial<ModelSettings>)}
              />
            ))}
            {model.supportsThinking && (
              <Field
                layout="inline"
                label="Think before answering"
                description="Slower, but better at multi-step problems. The thinking is shown collapsed above the answer."
              >
                <Switch checked={draft.thinking ?? model.effective.thinking ?? true} onChange={(v) => set({ thinking: v })} label="Thinking" />
              </Field>
            )}
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Length and memory</h3>
          <div className={`${s.card} ${s.cardPad} ${s.stack}`}>
            <div className={s.grid2}>
              <Field label="Longest answer" description="In tokens. Leave empty for no limit.">
                <TextField
                  type="number"
                  min={1}
                  value={draft.maxTokens ?? ''}
                  placeholder="No limit"
                  onChange={(e) => set({ maxTokens: e.target.value ? Math.max(1, Number(e.target.value)) : null })}
                />
              </Field>
              {local && (
                <Field
                  label="Context length"
                  description={`How much of the conversation the model can see. Trained up to ${maxContext.toLocaleString()} tokens.`}
                >
                  <Select
                    value={String(draft.contextLength ?? model.effective.contextLength ?? 8192)}
                    onChange={(e) => set({ contextLength: Number(e.target.value) })}
                    options={contextOptions.map((c) => ({ value: String(c), label: `${c.toLocaleString()} tokens` }))}
                  />
                </Field>
              )}
              {local && (
                <Field label="GPU layers" description="Auto fits as much of the model on the GPU as fits.">
                  <TextField
                    type="number"
                    min={0}
                    value={draft.gpuLayers ?? ''}
                    placeholder="Auto"
                    onChange={(e) => set({ gpuLayers: e.target.value === '' ? null : Math.max(0, Number(e.target.value)) })}
                  />
                </Field>
              )}
            </div>
          </div>
        </section>

        {local && (
          <section className={s.section}>
            <h3 className={s.sectionTitle}>Details</h3>
            <div className={`${s.card} ${s.cardPad}`}>
              <dl className={s.kv}>
                <dt>File</dt>
                <dd className={s.mono}>{model.path}</dd>
                {model.repo && (
                  <>
                    <dt>From</dt>
                    <dd>{model.repo}</dd>
                  </>
                )}
                <dt>Architecture</dt>
                <dd>{model.architecture ?? 'Unknown'}</dd>
                <dt>Quantization</dt>
                <dd>{model.quant ?? 'Unknown'}</dd>
                <dt>Trained context</dt>
                <dd>{model.maxContext ? `${model.maxContext.toLocaleString()} tokens` : 'Unknown'}</dd>
              </dl>
            </div>
          </section>
        )}
      </div>

      <div className={styles.footerBar}>
        <Button variant="ghost" icon={RotateCcw} onClick={() => void save({ enabled: draft.enabled, avatar: draft.avatar })}>
          Restore defaults
        </Button>
        <span className={styles.flex} />
        {dirty && (
          <Button variant="ghost" onClick={() => setDraft(model.settings)}>
            Discard changes
          </Button>
        )}
        <Button variant="primary" icon={Save} loading={saving} disabled={!dirty} onClick={() => void save(draft)}>
          Save
        </Button>
      </div>
    </div>
  );
}

function normalize(s: ModelSettings): ModelSettings {
  return Object.fromEntries(
    Object.entries(s).filter(([, v]) => v !== null && v !== undefined && v !== ''),
  ) as ModelSettings;
}

export type { ModelEntry };
