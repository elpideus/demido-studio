import { useEffect, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Cloud, ExternalLink, KeyRound, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { Badge, Button, Dialog, EmptyState, Field, Notice, Switch, TextField } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { formatDateTime } from '@/lib/format';
import type { ProviderView } from '@/lib/types';
import { toast } from '@/stores/toasts';
import s from './settings.module.css';
import styles from './ProvidersTab.module.css';

const KEY_URL = 'https://aistudio.google.com/apikey';

function KeyForm({
  submitLabel,
  onSubmit,
  onCancel,
}: {
  submitLabel: string;
  onSubmit: (key: string) => Promise<void>;
  onCancel?: () => void;
}) {
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSubmit(key.trim());
      setKey('');
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className={s.stack}>
      <Field
        label="API key"
        description={
          <>
            Create a free key in Google AI Studio.{' '}
            <a href={KEY_URL} onClick={(e) => { e.preventDefault(); void openUrl(KEY_URL); }}>
              Get a key <ExternalLink size={11} />
            </a>
          </>
        }
      >
        <TextField
          icon={KeyRound}
          type="password"
          placeholder="AIza…"
          value={key}
          invalid={!!error}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && key.trim() && void submit()}
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

function ProviderCard({ provider, onChange }: { provider: ProviderView; onChange: () => void }) {
  const [replacing, setReplacing] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  return (
    <div className={s.card}>
      <div className={styles.providerHead}>
        <span className={styles.providerIcon}>
          <Cloud size={20} strokeWidth={1.8} aria-hidden />
        </span>
        <div className={s.rowMain}>
          <div className={s.rowTitle}>
            {provider.name}
            {provider.hasKey ? <Badge tone="accent">Connected</Badge> : <Badge tone="warning">No key</Badge>}
          </div>
          <div className={s.rowMeta}>
            {provider.models.length} models available · key {provider.keyHint ?? 'missing'}
            {provider.modelsFetchedAt ? ` · checked ${formatDateTime(provider.modelsFetchedAt)}` : ''}
          </div>
        </div>
        <Switch
          checked={provider.enabled}
          label="Enabled"
          onChange={(enabled) =>
            void api.updateProvider(provider.id, { enabled }).then(onChange, (e) => toast.error('Could not update', errorText(e)))
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
              onChange();
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
                .then(() => {
                  toast.success('Model list refreshed');
                  onChange();
                }, (e) => toast.error('Could not refresh', errorText(e)))
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
  const load = () => void api.listProviders().then(setProviders);
  useEffect(load, []);

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
          {providers?.map((p) => <ProviderCard key={p.id} provider={p} onChange={load} />)}
          {providers && (providers.length === 0 || adding) && (
            <div className={s.card}>
              <div className={s.cardPad}>
                <div className={styles.addHead}>
                  <span className={styles.providerIcon}>
                    <Cloud size={20} strokeWidth={1.8} aria-hidden />
                  </span>
                  <div>
                    <div className={s.rowTitle}>Google Gemini</div>
                    <div className={s.rowMeta}>Fast, capable cloud models with a generous free tier.</div>
                  </div>
                </div>
                <KeyForm
                  submitLabel="Connect"
                  onCancel={providers.length > 0 ? () => setAdding(false) : undefined}
                  onSubmit={async (key) => {
                    const added = await api.addProvider(key);
                    toast.success('Gemini connected', `${added.models.length} models are available.`);
                    setAdding(false);
                    load();
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
