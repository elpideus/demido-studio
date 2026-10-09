import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Cloud, ExternalLink, KeyRound, Pencil, Plus, RefreshCw, Tag, Trash2 } from 'lucide-react';
import { Badge, Button, Dialog, EmptyState, Field, IconButton, Notice, Spinner, Switch, TextField } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { formatDateTime } from '@/lib/format';
import type { ProviderView } from '@/lib/types';
import { toast } from '@/stores/toasts';
import s from './settings.module.css';
import styles from './ProvidersTab.module.css';

const KEY_URL = 'https://aistudio.google.com/apikey';
const NAME_MAX = 60;

/** What each kind of provider is called when it has no custom name. */
const KIND_LABEL: Record<ProviderView['kind'], string> = { gemini: 'Google Gemini' };

function KeyForm({
  kind,
  submitLabel,
  withName = false,
  onSubmit,
  onCancel,
}: {
  submitLabel: string;
  /** Also ask for an optional name, for a provider being added. */
  withName?: boolean;
  onSubmit: (key: string, name: string) => Promise<void>;
  onCancel?: () => void;
}) {
  const id = useId();
  const [key, setKey] = useState('');
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSubmit(key.trim(), name.trim());
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
      {withName && (
        <Field label="Name" htmlFor={`${id}-name`} description="Shown in the model list. Optional.">
          <TextField
            id={`${id}-name`}
            icon={Tag}
            placeholder={KIND_LABEL.gemini}
            value={name}
            maxLength={NAME_MAX}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={submitOnEnter}
          />
        </Field>
      )}
      <Field
        label="API key"
        htmlFor={`${id}-key`}
        description={
          <>
            Create a free key in Google AI Studio.{' '}
            <a
              href={KEY_URL}
              onClick={(e) => {
                e.preventDefault();
                void openUrl(KEY_URL);
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
          placeholder="AIza…"
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
  const kindLabel = KIND_LABEL[provider.kind];

  useEffect(() => {
    if (renaming || !refocusRename.current) return;
    refocusRename.current = false;
    renameButton.current?.focus();
  }, [renaming]);

  const status = provider.hasKey ? <Badge tone="accent">Connected</Badge> : <Badge tone="warning">No key</Badge>;
  return (
    <div className={s.card}>
      <div className={styles.providerHead}>
        <span className={styles.providerIcon}>
          <Cloud size={20} strokeWidth={1.8} aria-hidden />
        </span>
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
            {provider.name !== kindLabel && `${kindLabel} · `}
            {provider.models.length} models available · key {provider.keyHint ?? 'missing'}
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
      {replacing ? (
        <div className={s.cardPad}>
          <KeyForm
            submitLabel="Save key"
            onCancel={() => setReplacing(false)}
            onSubmit={async (key) => {
              await api.updateProvider(provider.id, { apiKey: key });
              setReplacing(false);
              toast.success('Key updated');
              void onChange();
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

/** Cloud model providers. Google Gemini for now. */
export function ProvidersTab() {
  const [providers, setProviders] = useState<ProviderView[] | null>(null);
  const [adding, setAdding] = useState(false);
  const load = () => api.listProviders().then(setProviders);
  useEffect(() => void load(), []);

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
            <div className={s.card}>
              <div className={s.cardPad}>
                <div className={styles.addHead}>
                  <span className={styles.providerIcon}>
                    <Cloud size={20} strokeWidth={1.8} aria-hidden />
                  </span>
                  <div>
                    <div className={s.rowTitle}>{KIND_LABEL.gemini}</div>
                    <div className={s.rowMeta}>Fast, capable cloud models with a generous free tier.</div>
                  </div>
                </div>
                <KeyForm
                  submitLabel="Connect"
                  withName
                  onCancel={providers.length > 0 ? () => setAdding(false) : undefined}
                  onSubmit={async (key, name) => {
                    const added = await api.addProvider(key, name || undefined);
                    toast.success('Gemini connected', `${added.models.length} models are available.`);
                    setAdding(false);
                    void load();
                  }}
                />
              </div>
            </div>
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
