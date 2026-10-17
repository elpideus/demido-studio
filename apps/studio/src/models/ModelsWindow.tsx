import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowDownWideNarrow,
  ArrowUpNarrowWide,
  BadgeCheck,
  Boxes,
  Brain,
  Check,
  ChevronDown,
  Cpu,
  Download,
  Eye,
  FolderOpen,
  Heart,
  Lock,
  RefreshCw,
  Search,
  Wrench,
  X,
  type LucideIcon,
} from 'lucide-react';
import {
  Button,
  EmptyState,
  IconButton,
  Menu,
  type MenuEntry,
  Notice,
  Spinner,
  Tooltip,
  cx,
  formatCount,
  useDebounced,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { HfRepo, HfSort, MemoryBudget, Recommendation } from '@/lib/types';
import { CapabilityIcons } from '@/settings/models/Capabilities';
import { useModels } from '@/stores/models';
import type { WindowState } from '@/stores/windows';
import { isRunning, shortName } from '@/downloads/queue';
import { reserveBytes } from './fit';
import { formatParams } from './format';
import { RepoDetail } from './RepoDetail';
import styles from './ModelsWindow.module.css';

type Sort = HfSort | 'parameters' | 'name';
type Show = 'all' | 'installed';
type CapabilityFilter = 'vision' | 'tools' | 'thinking';

const SORTS: Array<{ id: Sort; label: string }> = [
  { id: 'recommended', label: 'Recommended' },
  { id: 'downloads', label: 'Most downloaded' },
  { id: 'likes', label: 'Most liked' },
  { id: 'trending', label: 'Trending' },
  { id: 'updated', label: 'Recently updated' },
  { id: 'parameters', label: 'Parameters' },
  { id: 'name', label: 'Name' },
];

const CAPABILITY_FILTERS: Array<{ key: CapabilityFilter; icon: LucideIcon; label: string }> = [
  { key: 'vision', icon: Eye, label: 'Only models that see images' },
  { key: 'tools', icon: Wrench, label: 'Only models that use tools' },
  { key: 'thinking', icon: Brain, label: 'Only models that think before answering' },
];

/** Publishers whose quantizations are made carefully and kept up to date. */
const KNOWN_QUANTIZERS = new Set(['unsloth', 'bartowski', 'lmstudio-community', 'ggml-org']);

/** Bits per weight a model is assumed to be downloaded at, to tell from its parameters whether it fits. */
const TYPICAL_BITS = 4.9;

export function isKnownQuantizer(author: string): boolean {
  return KNOWN_QUANTIZERS.has(author.toLowerCase());
}

/** A row for a repository the search did not return: what the id alone tells. */
function bareRepo(id: string): HfRepo {
  return {
    id,
    author: id.split('/')[0] ?? id,
    downloads: 0,
    likes: 0,
    lastModified: null,
    preferred: id.toLowerCase().startsWith('unsloth/'),
    gated: false,
    parameters: null,
    architecture: null,
    contextLength: null,
    license: null,
    pipeline: null,
    baseModel: null,
    capabilities: { vision: null, audio: null, tools: null, thinking: null },
  };
}

function matchesQuery(repo: HfRepo, query: string): boolean {
  const id = repo.id.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((term) => id.includes(term));
}

/**
 * Getting models: Hugging Face's GGUF repositories, searchable and sortable, on the left, and the
 * one picked on the right with its download options first and its details below.
 */
export function ModelsWindow({ win }: { win: WindowState }) {
  const models = useModels((st) => st.models);
  const downloads = useModels((st) => st.downloads);
  const [query, setQuery] = useState('');
  const debounced = useDebounced(query.trim(), 350);
  const [sort, setSort] = useState<Sort>('recommended');
  const [descending, setDescending] = useState(true);
  const [show, setShow] = useState<Show>('all');
  const [caps, setCaps] = useState<Partial<Record<CapabilityFilter, boolean>>>({});
  const [fitsOnly, setFitsOnly] = useState(false);
  const [results, setResults] = useState<HfRepo[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [selectedId, setSelectedId] = useState<string | null>(
    typeof win.props.repo === 'string' ? win.props.repo : null,
  );
  const [budget, setBudget] = useState<MemoryBudget | null>(null);
  const [picks, setPicks] = useState<Recommendation[]>([]);
  const [folder, setFolder] = useState<string | null>(null);
  const [sortOpen, setSortOpen] = useState(false);
  const sortRef = useRef<HTMLButtonElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void api.modelBudget().then(setBudget);
    void api.modelFolders().then((folders) => setFolder(folders[0]?.path ?? null));
  }, []);
  useEffect(() => {
    void api.recommendedModels().then(setPicks);
  }, [models]);
  useEffect(() => {
    if (typeof win.props.repo === 'string') setSelectedId(win.props.repo);
  }, [win.props.repo]);

  // What Hugging Face is asked for: the sorts it knows; the window's own sorts reorder its answer.
  const serverSort: HfSort = sort === 'parameters' || sort === 'name' ? 'recommended' : sort;
  useEffect(() => {
    let current = true;
    setSearching(true);
    setError(null);
    api
      .hfSearch(debounced, serverSort)
      .then(
        (found) => current && setResults(found),
        (e) => current && setError(errorText(e)),
      )
      .finally(() => current && setSearching(false));
    return () => {
      current = false;
    };
  }, [debounced, serverSort, attempt]);

  const installedRepos = useMemo(
    () => new Set(models.filter((m) => m.source === 'local' && m.repo).map((m) => m.repo!.toLowerCase())),
    [models],
  );
  const progress = useMemo(() => {
    const byRepo = new Map<string, { downloaded: number; total: number; moving: boolean }>();
    for (const job of downloads) {
      if (job.state === 'done') continue;
      const p = byRepo.get(job.repo) ?? { downloaded: 0, total: 0, moving: false };
      p.downloaded += job.downloaded;
      p.total += job.total;
      p.moving ||= isRunning(job);
      byRepo.set(job.repo, p);
    }
    return byRepo;
  }, [downloads]);

  const room = budget ? budget.memory - reserveBytes(budget) : null;
  const fits = (repo: HfRepo) =>
    room === null || repo.parameters === null || (repo.parameters * TYPICAL_BITS) / 8 <= room;

  const filtered = (repos: HfRepo[]) =>
    repos
      .filter((r) => show === 'all' || installedRepos.has(r.id.toLowerCase()))
      .filter((r) => (Object.keys(caps) as CapabilityFilter[]).every((k) => !caps[k] || r.capabilities[k] === true))
      .filter((r) => !fitsOnly || fits(r));

  // At most a search's worth of rows: cheap enough to work out on every render.
  const listed = (() => {
    let repos = results ?? [];
    // Installed repositories the search did not return are still listed under Installed.
    if (show === 'installed') {
      const missing = [...new Set(models.filter((m) => m.source === 'local' && m.repo).map((m) => m.repo!))]
        .filter((id) => !repos.some((r) => r.id.toLowerCase() === id.toLowerCase()))
        .map(bareRepo)
        .filter((r) => matchesQuery(r, debounced));
      repos = [...repos, ...missing];
    }
    repos = filtered(repos);
    const direction = descending ? 1 : -1;
    if (sort === 'parameters') {
      repos = [...repos].sort((a, b) => ((b.parameters ?? 0) - (a.parameters ?? 0)) * direction);
    } else if (sort === 'name') {
      repos = [...repos].sort((a, b) => shortName(a.id).localeCompare(shortName(b.id)) * -direction);
    } else if (!descending) {
      repos = [...repos].reverse();
    }
    return repos;
  })();

  // The curated starter models for this computer, first, while nothing narrows the list.
  const narrowed = debounced !== '' || show !== 'all' || fitsOnly || Object.values(caps).some(Boolean);
  const pickRepos = useMemo(() => {
    if (narrowed) return [];
    const ids = [...new Set(picks.map((p) => p.repo))];
    return ids.map((id) => results?.find((r) => r.id === id) ?? bareRepo(id));
  }, [narrowed, picks, results]);
  const rest = listed.filter((r) => !pickRepos.some((p) => p.id === r.id));
  const ordered = [...pickRepos, ...rest];

  const selected = ordered.find((r) => r.id === selectedId) ?? (selectedId ? bareRepo(selectedId) : undefined);
  // Something is always on the right while the list has rows.
  useEffect(() => {
    if (!selectedId && ordered[0]) setSelectedId(ordered[0].id);
  }, [selectedId, ordered]);

  const move = (step: number) => {
    const i = ordered.findIndex((r) => r.id === selectedId);
    const next = ordered[Math.max(0, Math.min(ordered.length - 1, i + step))];
    if (!next) return;
    setSelectedId(next.id);
    listRef.current?.querySelector(`[data-repo="${CSS.escape(next.id)}"]`)?.scrollIntoView({ block: 'nearest' });
  };
  const onListKey = (e: KeyboardEvent) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      move(e.key === 'ArrowDown' ? 1 : -1);
    }
  };
  const onWindowKey = (e: KeyboardEvent) => {
    const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
    if (e.key === '/' && !typing) {
      e.preventDefault();
      searchRef.current?.focus();
    }
  };

  const sortItems: MenuEntry[] = SORTS.map((s) => ({
    id: s.id,
    label: s.label,
    checked: s.id === sort,
    onSelect: () => {
      setSort(s.id);
      setDescending(s.id !== 'name');
    },
  }));

  const rowProps = (repo: HfRepo) => ({
    repo,
    selected: repo.id === selected?.id,
    installed: installedRepos.has(repo.id.toLowerCase()),
    progress: progress.get(repo.id),
    onSelect: () => setSelectedId(repo.id),
  });

  return (
    <div className={styles.window} onKeyDown={onWindowKey}>
      <div className={styles.list}>
        <div className={styles.listHead}>
          <label className={styles.search}>
            <Search size={15} strokeWidth={1.8} aria-hidden />
            <input
              ref={searchRef}
              className={styles.searchInput}
              placeholder="Search Hugging Face"
              aria-label="Search Hugging Face"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={onListKey}
            />
            {searching ? (
              <Spinner size={13} />
            ) : query ? (
              <IconButton icon={X} label="Clear" size="xs" tooltip={false} onClick={() => setQuery('')} />
            ) : (
              <kbd className={styles.kbd}>/</kbd>
            )}
          </label>
          <div className={styles.chips}>
            {(['all', 'installed'] as Show[]).map((s) => (
              <button
                key={s}
                type="button"
                className={styles.chip}
                aria-pressed={show === s}
                onClick={() => setShow(s)}
              >
                {s === 'all' ? 'All' : 'Installed'}
              </button>
            ))}
            <span className={styles.chipGap} />
            {CAPABILITY_FILTERS.map(({ key, icon: Icon, label }) => (
              <Tooltip key={key} content={label} delay={300}>
                <button
                  type="button"
                  className={cx(styles.chip, styles.iconChip)}
                  aria-pressed={caps[key] === true}
                  aria-label={label}
                  onClick={() => setCaps((c) => ({ ...c, [key]: !c[key] }))}
                >
                  <Icon size={13} strokeWidth={1.9} aria-hidden />
                </button>
              </Tooltip>
            ))}
            <Tooltip
              content={
                budget?.gpu ? `Small enough to run entirely on the ${budget.gpu}` : 'Small enough for this computer'
              }
              delay={300}
            >
              <button
                type="button"
                className={cx(styles.chip, styles.iconChip)}
                aria-pressed={fitsOnly}
                onClick={() => setFitsOnly((f) => !f)}
              >
                <Cpu size={13} strokeWidth={1.9} aria-hidden />
                Fits
              </button>
            </Tooltip>
          </div>
          <div className={styles.sortRow}>
            <span>{results ? `${ordered.length} ${ordered.length === 1 ? 'model' : 'models'}` : ''}</span>
            <span className={styles.sortControls}>
              <button
                ref={sortRef}
                type="button"
                className={styles.sortButton}
                aria-haspopup="menu"
                onClick={() => setSortOpen((o) => !o)}
              >
                {SORTS.find((s) => s.id === sort)?.label}
                <ChevronDown size={13} aria-hidden />
              </button>
              <IconButton
                icon={descending ? ArrowDownWideNarrow : ArrowUpNarrowWide}
                label={descending ? 'Descending' : 'Ascending'}
                size="xs"
                onClick={() => setDescending((d) => !d)}
              />
            </span>
          </div>
        </div>

        <div
          ref={listRef}
          className={styles.rows}
          role="listbox"
          aria-label="Models"
          tabIndex={0}
          onKeyDown={onListKey}
        >
          {error && (
            <div className={styles.listNotice}>
              <Notice
                tone="danger"
                title="Hugging Face did not answer"
                action={
                  <Button size="sm" variant="secondary" icon={RefreshCw} onClick={() => setAttempt((a) => a + 1)}>
                    Retry
                  </Button>
                }
              >
                {error}
              </Notice>
            </div>
          )}
          {!results && !error && (
            <div className={styles.listLoading}>
              <Spinner />
            </div>
          )}
          {pickRepos.length > 0 && (
            <>
              <h3 className={styles.group}>Picked for this computer</h3>
              {pickRepos.map((repo) => (
                <RepoRow key={repo.id} {...rowProps(repo)} />
              ))}
              <h3 className={styles.group}>Hugging Face</h3>
            </>
          )}
          {rest.map((repo) => (
            <RepoRow key={repo.id} {...rowProps(repo)} />
          ))}
          {results && ordered.length === 0 && (
            <EmptyState
              compact
              icon={Search}
              title={show === 'installed' ? 'No installed model matches' : 'Nothing matches'}
              description={
                debounced ? 'Try a shorter name, or an author like unsloth or bartowski.' : 'Loosen the filters above.'
              }
            />
          )}
        </div>

        {folder && (
          <Tooltip content="Open the folder" delay={400}>
            <button type="button" className={styles.folder} onClick={() => void api.openPath(folder)}>
              <FolderOpen size={13} strokeWidth={1.8} aria-hidden />
              <span className={styles.folderLabel}>Saves to</span>
              <span className={styles.folderPath}>{folder}</span>
            </button>
          </Tooltip>
        )}
      </div>

      <section className={styles.detail} aria-label="Model">
        {selected && budget ? (
          <RepoDetail key={selected.id} repo={selected} budget={budget} />
        ) : budget ? (
          <EmptyState icon={Boxes} title="Choose a model" description="Its download options and details appear here." />
        ) : (
          <div className={styles.listLoading}>
            <Spinner />
          </div>
        )}
      </section>

      <Menu open={sortOpen} onClose={() => setSortOpen(false)} anchorRef={sortRef} items={sortItems} width={190} />
    </div>
  );
}

function RepoRow({
  repo,
  selected,
  installed,
  progress,
  onSelect,
}: {
  repo: HfRepo;
  selected: boolean;
  installed: boolean;
  progress?: { downloaded: number; total: number; moving: boolean };
  onSelect: () => void;
}) {
  const fraction = progress && progress.total > 0 ? progress.downloaded / progress.total : null;
  const params = formatParams(repo.parameters);
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      data-repo={repo.id}
      className={styles.row}
      onClick={onSelect}
      tabIndex={-1}
    >
      <span className={styles.rowTop}>
        <span className={styles.rowName}>{shortName(repo.id)}</span>
        {repo.gated && <Lock size={12} className={styles.gated} aria-label="Gated" />}
        <span className={styles.grow} />
        {installed ? (
          <span className={cx(styles.pill, styles.pillAccent)}>
            <Check size={10} strokeWidth={2.6} aria-hidden />
            Installed
          </span>
        ) : (
          fraction !== null && <span className={styles.pill}>{Math.floor(fraction * 100)}%</span>
        )}
      </span>
      <span className={styles.rowAuthor}>
        {repo.author}
        {isKnownQuantizer(repo.author) && <BadgeCheck size={11} className={styles.verified} aria-hidden />}
      </span>
      <span className={styles.rowFoot}>
        {params && <span>{params}</span>}
        {repo.downloads > 0 && (
          <span className={styles.stat}>
            <Download size={11} aria-hidden />
            {formatCount(repo.downloads)}
          </span>
        )}
        {repo.likes > 0 && (
          <span className={styles.stat}>
            <Heart size={11} aria-hidden />
            {formatCount(repo.likes)}
          </span>
        )}
        <span className={styles.grow} />
        <CapabilityIcons capabilities={repo.capabilities} />
      </span>
      {fraction !== null && (
        <span className={styles.rowProgress}>
          <span
            className={cx(styles.rowProgressFill, !progress?.moving && styles.rowProgressPaused)}
            style={{ width: `${fraction * 100}%` }}
          />
        </span>
      )}
    </button>
  );
}
