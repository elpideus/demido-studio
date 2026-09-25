import { useEffect, useMemo, useRef, useState } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  Cpu,
  Download,
  EyeOff,
  FolderOpen,
  FolderPlus,
  MoreHorizontal,
  Pencil,
  Play,
  Star,
  Trash2,
  X,
} from 'lucide-react';
import {
  Avatar,
  Badge,
  Button,
  Dialog,
  EmptyState,
  IconButton,
  Menu,
  Switch,
  formatBytes,
  type MenuEntry,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { fileUrl } from '@/lib/format';
import type { ModelEntry, ModelFolder } from '@/lib/types';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import s from '../settings.module.css';
import styles from './Models.module.css';

function meta(m: ModelEntry): string {
  if (m.source !== 'local') {
    return [m.providerName ?? 'Cloud', m.maxContext ? `${Math.round(m.maxContext / 1000)}K context` : null]
      .filter(Boolean)
      .join(' · ');
  }
  return [m.parameters, m.quant, m.size ? formatBytes(m.size) : null, m.architecture].filter(Boolean).join(' · ');
}

function ModelRow({ model, onEdit, onDelete }: { model: ModelEntry; onEdit: () => void; onDelete: () => void }) {
  const runtime = useModels((st) => st.runtime);
  const [menuOpen, setMenuOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const loaded = runtime?.modelId === model.id && runtime.state === 'ready';
  const loading = runtime?.modelId === model.id && runtime.state === 'loading';

  const toggle = (enabled: boolean) =>
    void api
      .updateModel(model.id, { ...model.settings, enabled })
      .catch((e) => toast.error('Could not update', errorText(e)));

  const items: MenuEntry[] = [
    { id: 'edit', label: 'Edit', icon: Pencil, onSelect: onEdit },
    {
      id: 'default',
      label: 'Use for new chats',
      icon: Star,
      disabled: model.isDefault || !model.enabled,
      onSelect: () => void api.setDefaultModel(model.id),
    },
  ];
  if (model.source === 'local') {
    items.push({
      id: 'load',
      label: loaded ? 'Loaded' : 'Load now',
      icon: Play,
      disabled: loaded || loading,
      onSelect: () => void api.loadModel(model.id).catch((e) => toast.error('Could not load the model', errorText(e))),
    });
    if (model.path) {
      items.push({
        id: 'reveal',
        label: 'Show in folder',
        icon: FolderOpen,
        onSelect: () => void api.revealPath(model.path!),
      });
    }
    items.push('separator');
    items.push(
      model.removable
        ? { id: 'delete', label: 'Delete from disk', icon: Trash2, danger: true, onSelect: onDelete }
        : { id: 'hide', label: 'Hide (keeps the file)', icon: EyeOff, onSelect: () => toggle(false) },
    );
  }

  return (
    <div
      className={`${s.row} ${s.rowButton}`}
      onClick={onEdit}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => e.key === 'Enter' && onEdit()}
    >
      <Avatar name={model.name} src={fileUrl(model.avatarPath)} size={34} />
      <div className={s.rowMain}>
        <div className={s.rowTitle}>
          <span className={styles.name}>{model.name}</span>
          {model.isDefault && <Badge tone="accent">Default</Badge>}
          {loaded && <Badge tone="info">Loaded</Badge>}
          {loading && <Badge tone="info">Loading</Badge>}
        </div>
        <div className={s.rowMeta}>{meta(model)}</div>
      </div>
      <div className={s.rowActions} onClick={(e) => e.stopPropagation()}>
        <Switch checked={model.enabled} onChange={toggle} label={model.enabled ? 'Enabled' : 'Disabled'} />
        <IconButton
          ref={anchor}
          icon={MoreHorizontal}
          label="More"
          size="sm"
          tooltip={false}
          onClick={() => setMenuOpen(true)}
        />
        <Menu open={menuOpen} onClose={() => setMenuOpen(false)} anchorRef={anchor} items={items} width={210} />
      </div>
    </div>
  );
}

function Folders() {
  const [folders, setFolders] = useState<ModelFolder[]>([]);
  const [suggested, setSuggested] = useState<string[]>([]);
  const load = () => {
    void api.modelFolders().then(setFolders);
    void api.suggestedModelFolders().then(setSuggested);
  };
  useEffect(load, []);
  const add = async (path: string) => {
    try {
      useModels.getState().setModels(await api.addModelFolder(path));
      load();
    } catch (e) {
      toast.error('Could not add the folder', errorText(e));
    }
  };
  return (
    <section className={s.section}>
      <h3 className={s.sectionTitle}>
        Model folders
        <Button
          size="sm"
          variant="ghost"
          icon={FolderPlus}
          onClick={async () => {
            const picked = await openDialog({ directory: true, title: 'Add a folder with GGUF models' });
            if (typeof picked === 'string') void add(picked);
          }}
        >
          Add folder
        </Button>
      </h3>
      <div className={s.rows}>
        {folders.map((f) => (
          <div key={f.path} className={s.row}>
            <FolderOpen size={17} className={styles.folderIcon} aria-hidden />
            <div className={s.rowMain}>
              <div className={`${s.rowMeta} ${s.mono}`}>{f.path}</div>
            </div>
            {f.managed ? (
              <Badge>Demido</Badge>
            ) : (
              <IconButton
                icon={X}
                label="Stop scanning this folder"
                size="sm"
                onClick={async () => {
                  useModels.getState().setModels(await api.removeModelFolder(f.path));
                  load();
                }}
              />
            )}
          </div>
        ))}
        {suggested.map((path) => (
          <div key={path} className={s.row}>
            <FolderPlus size={17} className={styles.folderIcon} aria-hidden />
            <div className={s.rowMain}>
              <div className={s.rowTitle}>Models from LM Studio were found</div>
              <div className={`${s.rowMeta} ${s.mono}`}>{path}</div>
            </div>
            <Button size="sm" variant="secondary" onClick={() => void add(path)}>
              Use them
            </Button>
          </div>
        ))}
      </div>
    </section>
  );
}

export function ModelList({ onEdit, onDownload }: { onEdit: (id: string) => void; onDownload: () => void }) {
  const models = useModels((st) => st.models);
  const [deleting, setDeleting] = useState<ModelEntry | null>(null);
  const local = useMemo(() => models.filter((m) => m.source === 'local'), [models]);
  const cloudGroups = useMemo(() => {
    const map = new Map<string, ModelEntry[]>();
    for (const m of models.filter((x) => x.source !== 'local')) {
      const key = m.providerName ?? 'Cloud';
      map.set(key, [...(map.get(key) ?? []), m]);
    }
    return [...map.entries()].map(([name, list]) => ({
      name,
      list: [...list].sort((a, b) => Number(b.enabled) - Number(a.enabled) || a.name.localeCompare(b.name)),
    }));
  }, [models]);

  return (
    <div className={s.scroll}>
      <section className={s.section}>
        <h3 className={s.sectionTitle}>On this computer</h3>
        {local.length === 0 ? (
          <div className={s.card}>
            <EmptyState
              compact
              icon={Cpu}
              title="No local models yet"
              description="Download one to run privately on this computer, or add a folder that already has GGUF files."
              action={
                <Button variant="primary" icon={Download} onClick={onDownload}>
                  Download a model
                </Button>
              }
            />
          </div>
        ) : (
          <div className={s.rows}>
            {local.map((m) => (
              <ModelRow key={m.id} model={m} onEdit={() => onEdit(m.id)} onDelete={() => setDeleting(m)} />
            ))}
          </div>
        )}
      </section>

      {cloudGroups.map((group) => (
        <section key={group.name} className={s.section}>
          <h3 className={s.sectionTitle}>{group.name}</h3>
          <div className={s.rows}>
            {group.list.map((m) => (
              <ModelRow key={m.id} model={m} onEdit={() => onEdit(m.id)} onDelete={() => undefined} />
            ))}
          </div>
        </section>
      ))}

      <Folders />

      <Dialog
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        title={`Delete ${deleting?.name}?`}
        description={deleting?.size ? `This frees ${formatBytes(deleting.size)} of disk space.` : undefined}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              icon={Trash2}
              onClick={async () => {
                const target = deleting;
                setDeleting(null);
                if (!target) return;
                try {
                  useModels.getState().setModels(await api.deleteModel(target.id));
                  toast.success(`${target.name} was deleted`);
                } catch (e) {
                  toast.error('Could not delete the model', errorText(e));
                }
              }}
            >
              Delete
            </Button>
          </>
        }
      >
        The model file is removed from your computer. You can download it again later.
      </Dialog>
    </div>
  );
}
