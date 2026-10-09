import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Cloud, ExternalLink, KeyRound, Pencil, Plus, RefreshCw, Tag, Trash2, Waypoints } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import {
  Badge,
  Button,
  Dialog,
  EmptyState,
  Field,
  IconButton,
  Notice,
  SegmentedControl,
  Select,
  Spinner,
  Switch,
  TextField,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { formatDateTime } from '@/lib/format';
import type { ModelGroup, ProviderKind, ProviderView } from '@/lib/types';
import { useProviderUsage } from '@/stores/providerUsage';
import { toast } from '@/stores/toasts';
import s from './settings.module.css';
import styles from './ProvidersTab.module.css';
import { ProviderUsageMeters, useUsageRefresh } from './UsageMeter';

const NAME_MAX = 60;

/** What the person is told about each kind of provider while connecting it. */
interface Preset {
  /** What the provider is called when it has no custom name. */
  label: string;
  pitch: string;
  icon: LucideIcon;
  keyUrl: string;
  keyHelp: string;
  keyPlaceholder: string;
  /** Lists paid models next to free ones, so the person picks which it offers. */
  groups: boolean;
}

const PRESETS: Record<ProviderKind, Preset> = {
  gemini: {
    label: 'Google Gemini',
    pitch: 'Fast, capable cloud models with a generous free tier.',
    icon: Cloud,
    keyUrl: 'https://aistudio.google.com/apikey',
    keyHelp: 'Create a free key in Google AI Studio.',
    keyPlaceholder: 'AIza…',
    groups: false,
  },
  openrouter: {
    label: 'OpenRouter',
    pitch: 'Hundreds of models from every major lab through one key, some of them free.',
    icon: Waypoints,
    keyUrl: 'https://openrouter.ai/settings/keys',
    keyHelp: 'Create a key in your OpenRouter account.',
    keyPlaceholder: 'sk-or-…',
    groups: true,
  },
};

const KINDS = (Object.keys(PRESETS) as ProviderKind[]).map((k) => ({ value: k, label: PRESETS[k].label }));

const GROUPS: Array<{ value: ModelGroup; label: string }> = [
  { value: 'all', label: 'All models' },
  { value: 'free', label: 'Free models only' },
];

const GROUP_NOTE =
  'Free models cost nothing but have daily limits, and their providers may keep what you send. Paid models start switched off.';

/** "12 models", or "12 free models" when only the free ones are offered. */
function countModels(p: ProviderView): string {
  const n = p.models.length;
  return `${n} ${p.modelGroup === 'free' ? 'free ' : ''}model${n === 1 ? '' : 's'}`;
}

function ProviderIcon({ kind }: { kind: ProviderKind }) {
  const Icon = PRESETS[kind].icon;
  return (
    <span className={styles.providerIcon}>
      <Icon size={20} strokeWidth={1.8} aria-hidden />
    </span>
  );
}

function KeyForm({
  kind,
  submitLabel,
  adding = false,
  onSubmit,
  onCancel,
}: {
  kind: ProviderKind;
  submitLabel: string;
  /** Also ask for an optional name and, where there is a choice, which models to offer. */
  adding?: boolean;
  onSubmit: (key: string, name: string, group: ModelGroup) => Promise<void>;
  onCancel?: () => void;
}) {
  const id = useId();
  const preset = PRESETS[kind];
  const [key, setKey] = useState('');
  const [name, setName] = useState('');
  const [group, setGroup] = useState<ModelGroup>('all');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSubmit(key.trim(), name.trim(), group);
      setKey('');
      setName('');
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const submitOnEnter = (e: KeyboardEvent) => e.key === 'Enter' && key.trim() && !busy && void submit();
  return (
    <div className={s.stack}>
      {adding && (
        <Field label="Name" htmlFor={`${id}-name`} description="Shown in the model list. Optional.">
          <TextField
            id={`${id}-name`}
            icon={Tag}
            placeholder={preset.label}
            value={name}
            maxLength={NAME_MAX}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={submitOnEnter}
          />
        </Field>
      )}
      {adding && preset.groups && (
        <Field label="Models" htmlFor={`${id}-group`} description={GROUP_NOTE}>
          <Select
            id={`${id}-group`}
            options={GROUPS}
            value={group}
            onChange={(e) => setGroup(e.target.value as ModelGroup)}
          />
        </Field>
      )}
      <Field
        label="API key"
        htmlFor={`${id}-key`}
        description={
          <>
            {preset.keyHelp}{' '}
            <a
              href={preset.keyUrl}
              onClick={(e) => {
                e.preventDefault();
                void openUrl(preset.keyUrl);
              }}
            >
              Get a key <ExternalLink size={11} />
            </a>
          </>
        }
      >
        <TextField
          id={`${id}-key`}
          icon={KeyRound}
          type="password"
          placeholder={preset.keyPlaceholder}
          value={key}
          invalid={!!error}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={submitOnEnter}
          autoFocus
        />
      </Field>
      {error && <Notice tone="danger">{error}</Notice>}
      <div className={styles.formActions}>
        {onCancel && (
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
        )}
        <Button variant="primary" loading={busy} disabled={!key.trim()} onClick={() => void submit()}>
          {submitLabel}
        </Button>
      </div>
    </div>
  );
}

/** Which of OpenRouter's models a connected provider offers; changing it lists them again. */
function GroupSelect({ provider, onChange }: { provider: ProviderView; onChange: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  return (
    <Select
      size="sm"
      aria-label="Models"
      className={styles.groupSelect}
      options={GROUPS}
      value={provider.modelGroup}
      disabled={busy}
      onChange={(e) => {
        setBusy(true);
        api
          .updateProvider(provider.id, { modelGroup: e.target.value as ModelGroup })
          .then(
            (updated) => {
              toast.success('Model list updated', `${countModels(updated)} available.`);
              return onChange();
            },
            (err) => toast.error('Could not change the models', errorText(err)),
          )
          .finally(() => setBusy(false));
      }}
    />
  );
}

/** Inline field that renames a provider. Enter or leaving the field saves, Escape cancels. */
function NameField({
  provider,
  onChange,
  onDone,
}: {
  provider: ProviderView;
  onChange: () => Promise<void>;
  /** `refocus` is true when the field was closed from the keyboard, so focus goes back to the rename button. */
  onDone: (refocus: boolean) => void;
}) {
  const [value, setValue] = useState(provider.name);
  const [saving, setSaving] = useState(false);
  const finished = useRef(false);
  const finish = async (save: boolean, refocus: boolean) => {
    if (finished.current) return;
    finished.current = true;
    const name = value.trim();
    if (save && name && name !== provider.name) {
      setSaving(true);
      try {
        await api.updateProvider(provider.id, { name });
        await onChange();
      } catch (e) {
        toast.error('Could not rename the provider', errorText(e));
      }
    }
    onDone(refocus);
  };
  return (
    <TextField
      size="sm"
      className={styles.nameField}
      aria-label="Provider name"
      value={value}
      maxLength={NAME_MAX}
      disabled={saving}
      trailing={saving && <Spinner size={14} />}
      onChange={(e) => setValue(e.target.value)}
      onFocus={(e) => e.target.select()}
      onBlur={() => void finish(true, false)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') void finish(true, true);
        if (e.key === 'Escape') void finish(false, true);
      }}
      autoFocus
    />
  );
}

function ProviderCard({ provider, onChange }: { provider: ProviderView; onChange: () => Promise<void> }) {
  const [renaming, setRenaming] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const renameButton = useRef<HTMLButtonElement>(null);
  const refocusRename = useRef(false);
  const preset = PRESETS[provider.kind];

  useEffect(() => {
    if (renaming || !refocusRename.current) return;
    refocusRename.current = false;
    renameButton.current?.focus();
  }, [renaming]);

  const status = provider.hasKey ? <Badge tone="accent">Connected</Badge> : <Badge tone="warning">No key</Badge>;
  return (
    <div className={s.card}>
      <div className={styles.providerHead}>
        <ProviderIcon kind={provider.kind} />
        <div className={s.rowMain}>
          {renaming ? (
            <div className={styles.renameRow}>
              <NameField
                provider={provider}
                onChange={onChange}
                onDone={(refocus) => {
                  refocusRename.current = refocus;
                  setRenaming(false);
                }}
              />
              {status}
            </div>
          ) : (
            <div className={`${s.rowTitle} ${styles.title}`}>
              <span className={styles.name} title={provider.name} onDoubleClick={() => setRenaming(true)}>
                {provider.name}
              </span>
              <IconButton
                ref={renameButton}
                icon={Pencil}
                label="Rename"
                size="xs"
                className={styles.rename}
                onClick={() => setRenaming(true)}
              />
              {status}
            </div>
          )}
          <div className={s.rowMeta}>
            {provider.name !== preset.label && `${preset.label} · `}
            {countModels(provider)} available · key {provider.keyHint ?? 'missing'}
            {provider.modelsFetchedAt ? ` · checked ${formatDateTime(provider.modelsFetchedAt)}` : ''}
          </div>
        </div>
        <Switch
          checked={provider.enabled}
          label="Enabled"
          onChange={(enabled) =>
            void api
              .updateProvider(provider.id, { enabled })
              .then(onChange, (e) => toast.error('Could not update', errorText(e)))
          }
        />
      </div>
      {provider.hasKey && <ProviderUsageMeters providerId={provider.id} />}
      {replacing ? (
        <div className={s.cardPad}>
          <KeyForm
            kind={provider.kind}
            submitLabel="Save key"
            onCancel={() => setReplacing(false)}
            onSubmit={async (key) => {
              await api.updateProvider(provider.id, { apiKey: key });
              setReplacing(false);
              toast.success('Key updated');
              void onChange();
              void useProviderUsage.getState().refresh(true);
            }}
          />
        </div>
      ) : (
        <div className={styles.providerActions}>
          <Button size="sm" variant="secondary" icon={KeyRound} onClick={() => setReplacing(true)}>
            Replace key
          </Button>
          <Button
            size="sm"
            variant="secondary"
            icon={RefreshCw}
            loading={refreshing}
            onClick={() => {
              setRefreshing(true);
              api
                .refreshProvider(provider.id)
                .then(
                  () => {
                    toast.success('Model list refreshed');
                    void onChange();
                  },
                  (e) => toast.error('Could not refresh', errorText(e)),
                )
                .finally(() => setRefreshing(false));
            }}
          >
            Refresh models
          </Button>
          {preset.groups && <GroupSelect provider={provider} onChange={onChange} />}
          <span className={styles.flex} />
          <Button size="sm" variant="ghost" icon={Trash2} onClick={() => setRemoving(true)}>
            Remove
          </Button>
        </div>
      )}
      <Dialog
        open={removing}
        onClose={() => setRemoving(false)}
        title={`Remove ${provider.name}?`}
        description="Its API key is deleted from this computer and its models disappear from the model list."
        footer={
          <>
            <Button variant="ghost" onClick={() => setRemoving(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              icon={Trash2}
              onClick={() => {
                setRemoving(false);
                void api.removeProvider(provider.id).then(onChange);
              }}
            >
              Remove
            </Button>
          </>
        }
      />
    </div>
  );
}

/** What the person is told once a provider is connected. */
function connectedNote(p: ProviderView): string {
  if (p.kind !== 'openrouter' || p.modelGroup === 'free') return `${countModels(p)} are available.`;
  return `${countModels(p)} are available. The free ones are switched on; switch on others under Models.`;
}

/** A provider being connected: which kind, then its name, models and key. */
function AddCard({
  initialKind,
  onCancel,
  onAdded,
}: {
  initialKind: ProviderKind;
  onCancel?: () => void;
  onAdded: () => void;
}) {
  const [kind, setKind] = useState(initialKind);
  const preset = PRESETS[kind];
  return (
    <div className={s.card}>
      <div className={s.cardPad}>
        <SegmentedControl value={kind} onChange={setKind} options={KINDS} className={styles.kinds} />
        <div className={styles.addHead}>
          <ProviderIcon kind={kind} />
          <div>
            <div className={s.rowTitle}>{preset.label}</div>
            <div className={s.rowMeta}>{preset.pitch}</div>
          </div>
        </div>
        <KeyForm
          key={kind}
          kind={kind}
          submitLabel="Connect"
          adding
          onCancel={onCancel}
          onSubmit={async (key, name, group) => {
            const added = await api.addProvider(kind, key, name || undefined, preset.groups ? group : undefined);
            toast.success(`${preset.label} connected`, connectedNote(added));
            onAdded();
          }}
        />
      </div>
    </div>
  );
}

/** Cloud model providers: Google Gemini and OpenRouter. */
export function ProvidersTab() {
  const [providers, setProviders] = useState<ProviderView[] | null>(null);
  const [adding, setAdding] = useState(false);
  const load = () => api.listProviders().then(setProviders);
  useEffect(() => void load(), []);
  useUsageRefresh();
  // The first kind not connected yet, so adding a second provider starts on the other one.
  const nextKind = KINDS.find((k) => !providers?.some((p) => p.kind === k.value))?.value ?? 'gemini';

  return (
    <div className={s.page}>
      <header className={s.header}>
        <div className={s.headerText}>
          <h2 className={s.title}>Providers</h2>
          <p className={s.subtitle}>Connect cloud AI services. Their models appear next to your local ones.</p>
        </div>
        {providers && providers.length > 0 && !adding && (
          <div className={s.headerActions}>
            <Button icon={Plus} onClick={() => setAdding(true)}>
              Add provider
            </Button>
          </div>
        )}
      </header>
      <div className={s.scroll}>
        <div className={s.stack}>
          {providers?.map((p) => (
            <ProviderCard key={p.id} provider={p} onChange={load} />
          ))}
          {providers && (providers.length === 0 || adding) && (
            <AddCard
              initialKind={nextKind}
              onCancel={providers.length > 0 ? () => setAdding(false) : undefined}
              onAdded={() => {
                setAdding(false);
                void load();
                void useProviderUsage.getState().refresh(true);
              }}
            />
          )}
          {providers === null && <EmptyState compact title="Loading…" />}
          <Notice tone="info" icon={Cloud}>
            Conversations with cloud models are sent to the provider. Local models never leave this computer.
          </Notice>
        </div>
      </div>
    </div>
  );
}
