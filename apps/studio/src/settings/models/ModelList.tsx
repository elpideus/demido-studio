import { useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  ChevronRight,
  Cpu,
  Download,
  EyeOff,
  FolderOpen,
  FolderPlus,
  MoreHorizontal,
  Pencil,
  Play,
  Search,
  SearchX,
  Star,
  ToggleLeft,
  ToggleRight,
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
  TextField,
  cx,
  formatBytes,
  type MenuEntry,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { fileUrl } from '@/lib/format';
import type { ModelEntry, ModelFolder } from '@/lib/types';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import s from '../settings.module.css';
import { CapabilityIcons } from './Capabilities';
import { LOCAL_GROUP, filterModels, groupModels, type ModelGroupData } from './modelGroups';
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
      onKeyDown={(e) => e.key === 'Enter' && e.target === e.currentTarget && onEdit()}
    >
      <Avatar name={model.name} src={fileUrl(model.avatarPath)} size={34} />
      <div className={s.rowMain}>
        <div className={s.rowTitle}>
          <span className={styles.name}>{model.name}</span>
          {model.isDefault && <Badge tone="accent">Default</Badge>}
          {loaded && <Badge tone="info">Loaded</Badge>}
          {loading && <Badge tone="info">Loading</Badge>}
          <CapabilityIcons model={model} />
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

/** A search box whose X and Escape clear it, or close it when `onClose` is given. */
function SearchField({
  value,
  onChange,
  placeholder,
  onClose,
  size,
  inputRef,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  onClose?: () => void;
  size?: 'sm' | 'md';
  inputRef?: RefObject<HTMLInputElement | null>;
}) {
  const ownRef = useRef<HTMLInputElement>(null);
  const input = inputRef ?? ownRef;
  const dismissable = onClose !== undefined || value !== '';
  const dismiss = () => {
    if (onClose) {
      onClose();
    } else {
      onChange('');
      input.current?.focus();
    }
  };
  return (
    <TextField
      ref={input}
      icon={Search}
      size={size}
      value={value}
      placeholder={placeholder}
      aria-label={placeholder}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Escape' && dismissable) {
          e.stopPropagation();
          dismiss();
        }
      }}
      trailing={
        dismissable && (
          <IconButton
            icon={X}
            label={onClose ? 'Close search' : 'Clear search'}
            size="xs"
            tooltip={false}
            onClick={dismiss}
          />
        )
      }
    />
  );
}

/** Turns on or off every model a group shows; the labels say "shown" while a search narrows the group. */
function BulkActions({ models, narrowed }: { models: ModelEntry[]; narrowed: boolean }) {
  const [pending, setPending] = useState<'on' | 'off' | null>(null);
  const off = models.filter((m) => !m.enabled);
  const on = models.filter((m) => m.enabled);

  const setEnabled = async (enabled: boolean) => {
    setPending(enabled ? 'on' : 'off');
    try {
      const ids = (enabled ? off : on).map((m) => m.id);
      useModels.getState().setModels(await api.setModelsEnabled(ids, enabled));
    } catch (e) {
      toast.error(enabled ? 'Could not activate the models' : 'Could not deactivate the models', errorText(e));
    } finally {
      setPending(null);
    }
  };

  const label = (verb: string) => (narrowed ? `${verb} ${models.length} shown` : `${verb} all`);
  return (
    <>
      <Button
        size="sm"
        variant="ghost"
        icon={ToggleRight}
        loading={pending === 'on'}
        disabled={pending !== null || off.length === 0}
        onClick={() => void setEnabled(true)}
      >
        {label('Activate')}
      </Button>
      <Button
        size="sm"
        variant="ghost"
        icon={ToggleLeft}
        loading={pending === 'off'}
        disabled={pending !== null || on.length === 0}
        onClick={() => void setEnabled(false)}
      >
        {label('Deactivate')}
      </Button>
    </>
  );
}

/**
 * One section of the list, with its own search and bulk switches. It stays mounted while the
 * general search hides it, so its own search survives. Folded, it keeps its heading and actions
 * and lists only what a search finds, folding back once the search is cleared.
 */
function ModelGroup({
  group,
  matches,
  searching,
  expanded,
  onToggle,
  groupQuery,
  onGroupQueryChange: setGroupQuery,
  empty,
  onEdit,
  onDelete,
}: {
  group: ModelGroupData;
  /** The group's models that match the general search. */
  matches: ModelEntry[];
  searching: boolean;
  expanded: boolean;
  onToggle: () => void;
  /** The group's own search, or null while it is closed. */
  groupQuery: string | null;
  onGroupQueryChange: (query: string | null) => void;
  /** Shown in place of the rows when the group has no models at all. */
  empty?: ReactNode;
  onEdit: (id: string) => void;
  onDelete: (model: ModelEntry) => void;
}) {
  const searchButton = useRef<HTMLButtonElement>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const focusSearch = useRef(false);
  const searchOpen = groupQuery !== null;
  const shown = useMemo(() => filterModels(matches, groupQuery ?? ''), [matches, groupQuery]);

  // Focus when the person opens the search, not when it comes back with the list (after the
  // editor, or when a general search shows the group again).
  useEffect(() => {
    if (searchOpen && focusSearch.current) searchInput.current?.focus();
    focusSearch.current = false;
  }, [searchOpen]);

  if (searching && matches.length === 0) return null;

  const total = group.models.length;
  const active = group.models.filter((m) => m.enabled).length;
  const narrowed = searching || !!groupQuery?.trim();
  // An empty group always shows what stands in for its rows.
  const listed = total === 0 || expanded || narrowed;
  const closeSearch = () => {
    setGroupQuery(null);
    searchButton.current?.focus();
  };

  return (
    <section className={cx(s.section, styles.group)}>
      <div className={cx(styles.groupHead, total > 0 && styles.foldable)}>
        <h3 className={styles.groupTitle}>
          {total === 0 ? (
            <span className={styles.groupLabel}>
              <span className={styles.groupName}>{group.title}</span>
            </span>
          ) : (
            <button
              type="button"
              className={cx(styles.groupLabel, styles.groupToggle)}
              aria-expanded={expanded}
              onClick={onToggle}
            >
              <ChevronRight size={16} className={cx(styles.chevron, expanded && styles.chevronOpen)} aria-hidden />
              <span className={styles.groupName}>{group.title}</span>
              <span className={styles.groupCount}>{`${active} of ${total} active`}</span>
            </button>
          )}
        </h3>
        {total > 0 && (
          <div className={styles.groupActions}>
            <IconButton
              ref={searchButton}
              icon={Search}
              label={group.searchLabel}
              size="sm"
              active={searchOpen}
              onClick={() => {
                if (searchOpen) return closeSearch();
                focusSearch.current = true;
                setGroupQuery('');
              }}
            />
            <BulkActions models={shown} narrowed={narrowed} />
          </div>
        )}
      </div>
      {searchOpen && total > 0 && (
        <div className={styles.groupSearch}>
          <SearchField
            inputRef={searchInput}
            size="sm"
            value={groupQuery}
            onChange={setGroupQuery}
            placeholder={group.searchLabel}
            onClose={closeSearch}
          />
        </div>
      )}
      {listed && (
        <div className={styles.groupBody}>
          {total === 0 ? (
            empty
          ) : shown.length === 0 ? (
            <EmptyState
              compact
              icon={SearchX}
              title={`No models match “${groupQuery?.trim()}”`}
              className={styles.noMatch}
            />
          ) : (
            shown.map((m) => <ModelRow key={m.id} model={m} onEdit={() => onEdit(m.id)} onDelete={() => onDelete(m)} />)
          )}
        </div>
      )}
    </section>
  );
}

function NoLocalModels({ onDownload }: { onDownload: () => void }) {
  return (
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
  );
}

/** Stays mounted while `hidden`, so clearing a search does not reload the list. */
function Folders({ hidden }: { hidden: boolean }) {
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
    <section className={s.section} hidden={hidden}>
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

/** The Installed view: a search over every model above the scrolling groups and model folders. */
export function ModelList({
  query,
  onQueryChange,
  expandedGroups,
  onToggleGroup,
  groupQueries,
  onGroupQueryChange,
  onEdit,
  onDownload,
}: {
  query: string;
  onQueryChange: (query: string) => void;
  /** The groups listing all their models; the rest are folded. */
  expandedGroups: Record<string, boolean>;
  onToggleGroup: (group: string) => void;
  /** Each group's own search: null while closed. */
  groupQueries: Record<string, string | null>;
  onGroupQueryChange: (group: string, query: string | null) => void;
  onEdit: (id: string) => void;
  onDownload: () => void;
}) {
  const models = useModels((st) => st.models);
  const [deleting, setDeleting] = useState<ModelEntry | null>(null);
  const groups = useMemo(() => groupModels(models), [models]);
  const matched = useMemo(
    () => groups.map((group) => ({ group, matches: filterModels(group.models, query) })),
    [groups, query],
  );
  const searching = query.trim() !== '';
  const nothingFound = searching && matched.every((g) => g.matches.length === 0);

  return (
    <>
      <div className={styles.searchBar}>
        <SearchField value={query} onChange={onQueryChange} placeholder="Search all models" />
      </div>
      <div className={s.scroll}>
        {nothingFound && (
          <div className={cx(s.section, s.card)}>
            <EmptyState
              compact
              icon={SearchX}
              title={`No models match “${query.trim()}”`}
              description="Check the spelling, or try fewer words."
              className={styles.noMatch}
            />
          </div>
        )}

        {matched.map(({ group, matches }) => (
          <ModelGroup
            key={group.id}
            group={group}
            matches={matches}
            searching={searching}
            expanded={!!expandedGroups[group.id]}
            onToggle={() => onToggleGroup(group.id)}
            groupQuery={groupQueries[group.id] ?? null}
            onGroupQueryChange={(value) => onGroupQueryChange(group.id, value)}
            empty={group.id === LOCAL_GROUP && <NoLocalModels onDownload={onDownload} />}
            onEdit={onEdit}
            onDelete={setDeleting}
          />
        ))}

        <Folders hidden={searching} />

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
    </>
  );
}
