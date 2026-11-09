import { useCallback, useEffect, useState } from 'react';
import { Select, Switch, TextField } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { ModelEntry, Settings } from '@/lib/types';
import { useApp } from '@/stores/app';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { Group, PageHeader, Row } from './layout';
import s from './settings.module.css';

export function ChatsTab() {
  const settings = useApp((st) => st.settings);
  const models = useModels((st) => st.models);
  if (!settings) return null;
  const model = models.find((m) => m.isDefault && m.enabled) ?? models.find((m) => m.enabled);
  return (
    <div className={s.page}>
      <PageHeader page="chats" />
      <div className={s.scroll}>
        <Group
          title="New chats"
          footer="Any chat can switch to another model from the model button in the message box."
        >
          <Row
            setting="default-model"
            label="Model"
            description="What a new chat answers with. Turn models on in Settings, Models to list them here."
          >
            <DefaultModel models={models} current={model} />
          </Row>
        </Group>
        <AutoCompact settings={settings} model={model} />
      </div>
    </div>
  );
}

function DefaultModel({ models, current }: { models: ModelEntry[]; current: ModelEntry | undefined }) {
  const enabled = models.filter((m) => m.enabled);
  if (!enabled.length) {
    return (
      <Select
        size="sm"
        className={s.wide}
        aria-label="Model for new chats"
        disabled
        value=""
        options={[{ value: '', label: 'No model is turned on' }]}
      />
    );
  }
  const options = enabled.map((m) => ({
    value: m.id,
    label: m.source === 'local' ? m.name : `${m.name} · ${m.providerName ?? 'Cloud'}`,
  }));
  return (
    <Select
      size="sm"
      className={s.wide}
      aria-label="Model for new chats"
      value={current?.id ?? ''}
      onChange={(e) =>
        void api.setDefaultModel(e.target.value).then(
          (list) => useModels.getState().setModels(list),
          (err) => toast.error('Could not change the model for new chats', errorText(err)),
        )
      }
      options={options}
    />
  );
}

/**
 * When long chats are summarized to fit the model's context window. The threshold goes through
 * `/autocompact`, which reads it however it is written and says what it means for the model.
 */
function AutoCompact({ settings, model }: { settings: Settings; model: ModelEntry | undefined }) {
  const patch = useApp((st) => st.patchSettings);
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
    <Group
      title="Long chats"
      footer="A model can have its own threshold, in its settings under Settings, Models. Type /compact in a chat to compact it now."
    >
      <Row
        setting="auto-compact"
        label="Compact automatically"
        description={
          meaning ?? "Summarizes the earlier conversation when a chat nears the end of the model's context window."
        }
      >
        <Switch
          checked={settings.autoCompact ?? true}
          onChange={(v) => void patch({ autoCompact: v })}
          label="Compact long chats automatically"
        />
      </Row>
      <Row
        setting="compact-at"
        label="Compact at"
        description="Tokens, such as 12000, 12k or 12.5k. Empty, a chat is compacted just before the context window is full."
      >
        <TextField
          size="sm"
          className={s.narrow}
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
      </Row>
    </Group>
  );
}
