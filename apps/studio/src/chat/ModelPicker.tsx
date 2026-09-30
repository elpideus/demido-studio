import { useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, Cloud, Settings2 } from 'lucide-react';
import { Avatar, Popover, SearchList, formatBytes, type SearchListItem } from '@demido/ui';

import { fileUrl } from '@/lib/format';
import type { ModelEntry } from '@/lib/types';
import { CapabilityIcons } from '@/settings/models/Capabilities';
import { useModels } from '@/stores/models';
import { useWindows } from '@/stores/windows';
import styles from './Composer.module.css';

function describe(m: ModelEntry): string {
  if (m.source === 'gemini') {
    const ctx = m.maxContext ? `${Math.round(m.maxContext / 1000)}K context` : 'Cloud';
    return `${m.providerName ?? 'Gemini'} · ${ctx}`;
  }
  return [m.parameters, m.quant, m.size ? formatBytes(m.size) : null].filter(Boolean).join(' · ');
}

interface Props {
  model: ModelEntry | undefined;
  onPick: (id: string) => void;
}

/** The composer's model chip and its searchable list. */
export function ModelPicker({ model, onPick }: Props) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const models = useModels((s) => s.models);
  const runtime = useModels((s) => s.runtime);
  const openWindow = useWindows((s) => s.open);

  const items: SearchListItem[] = useMemo(
    () =>
      models
        .filter((m) => m.enabled)
        .map((m) => ({
          id: m.id,
          label: m.name,
          description: describe(m),
          keywords: `${m.defaultName} ${m.repo ?? ''} ${m.quant ?? ''} ${m.source}`,
          group: m.source === 'local' ? 'On this computer' : (m.providerName ?? 'Cloud'),
          leading: <Avatar name={m.name} src={fileUrl(m.avatarPath)} size={26} />,
          trailing: (
            <>
              <CapabilityIcons model={m} />
              {runtime?.state === 'ready' && runtime.modelId === m.id && (
                <span className={styles.loadedTag}>Loaded</span>
              )}
              {m.source !== 'local' && <Cloud size={13} aria-hidden />}
              {m.id === model?.id && <Check size={15} className={styles.checkIcon} aria-label="Selected" />}
            </>
          ),
        })),
    [models, runtime, model?.id],
  );

  return (
    <>
      <button
        ref={anchor}
        type="button"
        className={styles.chip}
        onClick={() => setOpen(!open)}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label="Choose model"
      >
        {model ? (
          <Avatar name={model.name} src={fileUrl(model.avatarPath)} size={20} />
        ) : (
          <span className={styles.chipDot} />
        )}
        <span className={styles.chipLabel}>{model?.name ?? 'Choose a model'}</span>
        <ChevronDown size={14} aria-hidden />
      </button>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        anchorRef={anchor}
        placement="top-start"
        width={380}
        aria-label="Models"
      >
        <SearchList
          items={items}
          selectedId={model?.id}
          placeholder="Search models"
          emptyText="No models are ready. Download one or connect a provider in Settings."
          onSelect={(item) => {
            onPick(item.id);
            setOpen(false);
          }}
          footer={{
            icon: Settings2,
            label: 'Settings',
            description: 'Manage, download and configure models',
            onSelect: () => {
              setOpen(false);
              openWindow('settings', { tab: 'models' });
            },
          }}
        />
      </Popover>
    </>
  );
}
