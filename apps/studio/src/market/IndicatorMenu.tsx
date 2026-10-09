import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import {
  Check,
  ChevronLeft,
  Code2,
  FileCode,
  LayoutTemplate,
  Library,
  LogIn,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  SquarePen,
  Star,
  Users,
  type LucideIcon,
} from 'lucide-react';
import {
  Badge,
  Button,
  Checkbox,
  IconButton,
  Notice,
  Popover,
  Spinner,
  TextField,
  cx,
  matchesQuery,
  useDebounced,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type {
  ChartLayout,
  ChartLayoutEntry,
  ChartLayoutStudy,
  IndicatorCatalog,
  IndicatorEntry,
  PineSummary,
} from '@/lib/types';
import { useMarket } from '@/stores/market';
import { usePine } from '@/stores/pine';
import { LOCAL_ENTRIES } from './indicators';
import styles from './MarketWindow.module.css';

type Section = 'favorites' | 'mine' | 'builtins' | 'community' | 'layouts' | 'pine';

const SECTIONS: Array<{ id: Section; label: string; icon: LucideIcon }> = [
  { id: 'favorites', label: 'Favorites', icon: Star },
  { id: 'mine', label: 'My scripts', icon: FileCode },
  { id: 'builtins', label: 'Built-ins', icon: Library },
  { id: 'community', label: 'Community', icon: Users },
  { id: 'layouts', label: 'Chart layouts', icon: LayoutTemplate },
  { id: 'pine', label: 'Pine scripts', icon: Code2 },
];

/** Whether TradingView shares a script's source, so it can be opened in the Pine Editor. */
const editable = (e: IndicatorEntry) =>
  e.kind !== 'strategy' && (e.id.startsWith('USER;') || (e.id.startsWith('PUB;') && (!e.access || e.access === 'open')));

// Kept between openings (the sidecar caches them too): the menu opens on what was there.
let catalogCache: IndicatorCatalog | null = null;
let layoutsCache: ChartLayoutEntry[] | null = null;

const ACCESS: Record<NonNullable<IndicatorEntry['access']>, string | null> = {
  open: null,
  protected: 'Closed source',
  invite: 'Invite-only',
};

export interface IndicatorMenuProps {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  loggedIn: boolean;
  onAdd: (entry: IndicatorEntry) => void;
  /** A layout's indicators to add; `replace` takes the chart's own indicators off first. */
  onImport: (layout: ChartLayout, studies: ChartLayoutStudy[], replace: boolean) => void;
  /** A script of Demido's Pine library to add. */
  onAddPine: (script: PineSummary) => void;
  /** Opens the Pine Editor on a library script, or on a new one (null). */
  onEditPine: (id: string | null) => void;
  /** Opens the Pine Editor on a copy of a TradingView script's source. */
  onOpenSource: (entry: IndicatorEntry) => void;
}

/** The Indicators menu: the user's TradingView favorites, scripts and chart layouts, TradingView's
 *  built-ins and community scripts, and the Pine scripts written here; signed out, the
 *  indicators computed on this computer. */
export function IndicatorMenu({
  open,
  onClose,
  anchorRef,
  loggedIn,
  onAdd,
  onImport,
  onAddPine,
  onEditPine,
  onOpenSource,
}: IndicatorMenuProps) {
  const pine = usePine((s) => s.scripts);
  const [catalog, setCatalog] = useState<IndicatorCatalog | null>(catalogCache);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [section, setSection] = useState<Section | null>(null);
  const [query, setQuery] = useState('');
  const debounced = useDebounced(query.trim(), 300);
  const [community, setCommunity] = useState<{ query: string; entries: IndicatorEntry[]; error?: string } | null>(null);
  const [added, setAdded] = useState<ReadonlySet<string>>(new Set());
  const search = useRef<HTMLInputElement>(null);

  const load = (refresh: boolean) => {
    setRefreshing(true);
    setCatalogError(null);
    api
      .marketIndicatorCatalog(refresh)
      .then(
        (c) => {
          catalogCache = c;
          setCatalog(c);
        },
        (e) => setCatalogError(errorText(e)),
      )
      .finally(() => setRefreshing(false));
  };

  // Another account may sign in next: its catalog and layouts are its own.
  useEffect(() => {
    if (loggedIn) return;
    catalogCache = null;
    layoutsCache = null;
    setCatalog(null);
    setCommunity(null);
  }, [loggedIn]);

  useEffect(() => {
    if (!open) return;
    setAdded(new Set());
    window.setTimeout(() => search.current?.focus(), 0);
    if (loggedIn && !catalog) load(false);
    // Opening is what loads it; a catalog already shown stays until refreshed.
  }, [open, loggedIn]);

  useEffect(() => {
    if (!open || !loggedIn || !debounced) return;
    let cancelled = false;
    api.marketIndicatorSearch(debounced).then(
      (entries) => !cancelled && setCommunity({ query: debounced, entries }),
      (e) => !cancelled && setCommunity({ query: debounced, entries: [], error: errorText(e) }),
    );
    return () => {
      cancelled = true;
    };
  }, [open, loggedIn, debounced]);

  const add = (entry: IndicatorEntry) => {
    onAdd(entry);
    setAdded((prev) => new Set(prev).add(entry.id));
  };

  const addPine = (script: PineSummary) => {
    onAddPine(script);
    setAdded((prev) => new Set(prev).add(`DEMIDO;${script.id}`));
  };

  const edit = (id: string | null) => {
    onEditPine(id);
    onClose();
  };

  const shown: Section = section ?? (catalog?.favorites.length ? 'favorites' : 'builtins');
  const q = query.trim();

  const list = (entries: readonly IndicatorEntry[], empty: string) =>
    entries.length ? (
      entries.map((e) => (
        <EntryRow
          key={`${e.id}@${e.version ?? ''}`}
          entry={e}
          added={added.has(e.id)}
          onAdd={add}
          onOpenSource={
            loggedIn && editable(e)
              ? () => {
                  onOpenSource(e);
                  onClose();
                }
              : undefined
          }
        />
      ))
    ) : (
      <div className={styles.resultsEmpty}>{empty}</div>
    );

  const matching = (entries: readonly IndicatorEntry[]) =>
    entries.filter((e) => matchesQuery(`${e.name} ${e.short ?? ''} ${e.author ?? ''}`, q));

  const pineList = (scripts: readonly PineSummary[], empty: string) =>
    scripts.length ? (
      scripts.map((s) => (
        <PineRow key={s.id} script={s} added={added.has(`DEMIDO;${s.id}`)} onAdd={addPine} onEdit={() => edit(s.id)} />
      ))
    ) : (
      <div className={styles.resultsEmpty}>{empty}</div>
    );

  const pineMatching = pine.filter((s) => matchesQuery(s.name, q));

  const pineSection = (
    <div className={styles.results}>
      <div className={styles.pineIntro}>
        <span>
          Written in Demido Studio, by you or the assistant. TradingView compiles and runs them; they stay on this
          computer until you save one to your account.
        </span>
        <Button size="sm" variant="secondary" icon={Plus} onClick={() => edit(null)}>
          New script
        </Button>
      </div>
      {pineList(pine, 'No Pine scripts yet. Write one, or open one of your TradingView scripts in the editor.')}
    </div>
  );

  let body: ReactNode;
  if (!loggedIn) {
    body = (
      <div className={styles.results}>
        <Notice
          tone="info"
          icon={LogIn}
          title="Sign in to TradingView for your indicators"
          action={
            <Button size="sm" variant="primary" icon={LogIn} onClick={() => void useMarket.getState().login()}>
              Sign in
            </Button>
          }
        >
          Your favorites, your scripts, your chart layouts and the community library come from your TradingView
          account. Meanwhile, these are computed on this computer from the chart's bars.
        </Notice>
        <div className={styles.resultsTitle}>Computed on this computer</div>
        {list(q ? matching(LOCAL_ENTRIES) : LOCAL_ENTRIES, 'No indicator matches.')}
        <div className={styles.resultsTitle}>Pine scripts · they run once you sign in</div>
        {pineList(q ? pineMatching : pine, q ? 'No Pine script matches.' : 'No Pine scripts yet.')}
        {!q && (
          <div className={styles.indicatorImport}>
            <Button size="sm" variant="secondary" icon={Plus} onClick={() => edit(null)}>
              New Pine script
            </Button>
          </div>
        )}
      </div>
    );
  } else if (q) {
    const groups: Array<[string, IndicatorEntry[]]> = catalog
      ? [
          ['Favorites', matching(catalog.favorites)],
          ['My scripts', matching(catalog.mine)],
          ['Built-ins', matching(catalog.builtins)],
        ]
      : [];
    const own = new Set(groups.flatMap(([, g]) => g.map((e) => e.id)));
    const fresh = community?.query === debounced ? community : null;
    const others = fresh?.entries.filter((e) => !own.has(e.id)) ?? [];
    body = (
      <div className={styles.results}>
        {pineMatching.length > 0 && (
          <div>
            <div className={styles.resultsTitle}>Pine scripts</div>
            {pineList(pineMatching, '')}
          </div>
        )}
        {groups
          .filter(([, g]) => g.length)
          .map(([title, g]) => (
            <div key={title}>
              <div className={styles.resultsTitle}>{title}</div>
              {list(g, '')}
            </div>
          ))}
        <div className={styles.resultsTitle}>Community</div>
        {!fresh ? (
          <div className={styles.resultsEmpty}>
            <Spinner size={14} />
          </div>
        ) : fresh.error ? (
          <div className={styles.resultsEmpty}>{fresh.error}</div>
        ) : (
          list(others, 'No community script matches.')
        )}
      </div>
    );
  } else if (shown === 'layouts') {
    body = <Layouts onImport={onImport} />;
  } else if (shown === 'pine') {
    body = pineSection;
  } else if (shown === 'community') {
    body = (
      <div className={styles.results}>
        <div className={styles.resultsEmpty}>Search above for scripts the TradingView community published.</div>
      </div>
    );
  } else {
    const entries = catalog?.[shown] ?? [];
    const empty =
      shown === 'favorites'
        ? 'No favorites yet. Star indicators on tradingview.com and they show up here.'
        : shown === 'mine'
          ? 'No scripts of your own yet.'
          : 'No built-in indicators came back from TradingView.';
    body = (
      <div className={styles.results}>
        {!catalog ? (
          <div className={styles.resultsEmpty}>{catalogError ?? <Spinner size={14} />}</div>
        ) : (
          list(entries, empty)
        )}
      </div>
    );
  }

  return (
    <Popover open={open} onClose={onClose} anchorRef={anchorRef} placement="bottom-start" width={600} maxHeight={480}>
      <div className={styles.indicatorMenu}>
        <div className={styles.indicatorMenuSearch}>
          <TextField
            ref={search}
            icon={Search}
            size="sm"
            placeholder={loggedIn ? 'Search indicators and community scripts' : 'Search indicators'}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {loggedIn && (
            <IconButton
              icon={RefreshCw}
              label="Reload from TradingView"
              size="sm"
              disabled={refreshing}
              onClick={() => {
                layoutsCache = null;
                load(true);
              }}
            />
          )}
        </div>
        <div className={styles.indicatorMenuBody}>
          {loggedIn && (
            <nav className={styles.indicatorMenuNav}>
              {SECTIONS.map((s) => (
                <button
                  key={s.id}
                  type="button"
                  className={cx(styles.indicatorMenuSection, !q && shown === s.id && styles.indicatorMenuActive)}
                  onClick={() => {
                    setSection(s.id);
                    setQuery('');
                  }}
                >
                  <s.icon size={14} aria-hidden />
                  {s.label}
                </button>
              ))}
            </nav>
          )}
          <div className={styles.indicatorMenuList}>{body}</div>
        </div>
      </div>
    </Popover>
  );
}

function EntryRow({
  entry,
  added,
  onAdd,
  onOpenSource,
}: {
  entry: IndicatorEntry;
  added: boolean;
  onAdd: (e: IndicatorEntry) => void;
  /** Present when its source can be opened in the Pine Editor. */
  onOpenSource?: () => void;
}) {
  const strategy = entry.kind === 'strategy';
  const access = entry.access ? ACCESS[entry.access] : null;
  return (
    <div className={styles.indicatorEntryRow}>
      <button
        type="button"
        className={styles.indicatorEntry}
        disabled={strategy}
        title={strategy ? 'Strategies are not supported yet.' : `Add ${entry.name} to the chart`}
        onClick={() => onAdd(entry)}
      >
        <span className={styles.indicatorEntryName}>{entry.name}</span>
        <span className={styles.resultMeta}>{entry.author ?? (entry.short && entry.short !== entry.name ? entry.short : '')}</span>
        <span className={styles.indicatorEntryBadges}>
          {strategy && <Badge>Strategy</Badge>}
          {access && <Badge tone="warning">{access}</Badge>}
          {added && <Check size={14} className={styles.indicatorAdded} aria-label="Added" />}
        </span>
      </button>
      {onOpenSource && (
        <IconButton
          icon={SquarePen}
          label={entry.id.startsWith('USER;') ? 'Open in the Pine Editor' : 'Open a copy in the Pine Editor'}
          size="sm"
          className={styles.indicatorEntryAction}
          onClick={onOpenSource}
        />
      )}
    </div>
  );
}

function PineRow({
  script,
  added,
  onAdd,
  onEdit,
}: {
  script: PineSummary;
  added: boolean;
  onAdd: (s: PineSummary) => void;
  onEdit: () => void;
}) {
  const runs = script.kind === 'indicator';
  const tv = script.tradingview;
  return (
    <div className={styles.indicatorEntryRow}>
      <button
        type="button"
        className={styles.indicatorEntry}
        disabled={!runs}
        title={
          runs
            ? `Add ${script.name} to the chart`
            : script.kind
              ? `A ${script.kind} does not go on a chart by itself.`
              : 'It has not compiled yet: open it in the editor.'
        }
        onClick={() => onAdd(script)}
      >
        <span className={styles.indicatorEntryName}>{script.name}</span>
        <span className={styles.resultMeta}>
          {script.lines} {script.lines === 1 ? 'line' : 'lines'}
        </span>
        <span className={styles.indicatorEntryBadges}>
          {script.kind && script.kind !== 'indicator' && <Badge>{script.kind === 'strategy' ? 'Strategy' : 'Library'}</Badge>}
          {tv && (
            <Badge tone={tv.changed ? 'warning' : 'accent'} title={tv.changed ? 'Changed since it was saved to TradingView' : 'Saved to your TradingView account'}>
              {tv.changed ? 'TradingView: changed' : 'On TradingView'}
            </Badge>
          )}
          {added && <Check size={14} className={styles.indicatorAdded} aria-label="Added" />}
        </span>
      </button>
      <IconButton icon={Pencil} label="Edit in the Pine Editor" size="sm" className={styles.indicatorEntryAction} onClick={onEdit} />
    </div>
  );
}

/** The chart layouts saved on the user's TradingView account, and the indicators of one of them. */
function Layouts({ onImport }: { onImport: IndicatorMenuProps['onImport'] }) {
  const [layouts, setLayouts] = useState<ChartLayoutEntry[] | null>(layoutsCache);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<ChartLayoutEntry | null>(null);
  const [layout, setLayout] = useState<ChartLayout | null>(null);
  const [layoutError, setLayoutError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<ReadonlySet<number>>(new Set());

  useEffect(() => {
    if (layouts) return;
    api.marketIndicatorLayouts().then(
      (list) => {
        layoutsCache = list;
        setLayouts(list);
      },
      (e) => setError(errorText(e)),
    );
  }, [layouts]);

  useEffect(() => {
    if (!picked) return;
    let cancelled = false;
    setLayout(null);
    setLayoutError(null);
    api.marketIndicatorLayout(picked.id).then(
      (l) => {
        if (cancelled) return;
        setLayout(l);
        setChosen(new Set(l.studies.map((_, i) => i)));
      },
      (e) => !cancelled && setLayoutError(errorText(e)),
    );
    return () => {
      cancelled = true;
    };
  }, [picked]);

  if (picked) {
    const studies = layout?.studies.filter((_, i) => chosen.has(i)) ?? [];
    return (
      <div className={styles.results}>
        <button type="button" className={styles.indicatorBack} onClick={() => setPicked(null)}>
          <ChevronLeft size={14} aria-hidden /> {picked.name}
        </button>
        {!layout ? (
          <div className={styles.resultsEmpty}>{layoutError ?? <Spinner size={14} />}</div>
        ) : (
          <>
            <div className={styles.resultsTitle}>
              {layout.symbol} · {layout.interval}
            </div>
            {layout.studies.length === 0 && <div className={styles.resultsEmpty}>This layout has no indicators.</div>}
            {layout.studies.map((s, i) => (
              <div key={i} className={styles.indicatorStudy}>
                <Checkbox
                  checked={chosen.has(i)}
                  onChange={(on) =>
                    setChosen((prev) => {
                      const next = new Set(prev);
                      if (on) next.add(i);
                      else next.delete(i);
                      return next;
                    })
                  }
                  label={s.name}
                />
                {s.hidden && <Badge title="Hidden in the layout; it is added hidden too">Hidden</Badge>}
              </div>
            ))}
            {layout.skipped.length > 0 && (
              <div className={styles.indicatorSkipped}>
                Not brought over (TradingView's older built-ins): {layout.skipped.join(', ')}
              </div>
            )}
            {layout.studies.length > 0 && (
              <div className={styles.indicatorImport}>
                <Button size="sm" variant="secondary" disabled={!studies.length} onClick={() => onImport(layout, studies, true)}>
                  Replace the chart's indicators
                </Button>
                <Button size="sm" variant="primary" disabled={!studies.length} onClick={() => onImport(layout, studies, false)}>
                  Add to chart
                </Button>
              </div>
            )}
          </>
        )}
      </div>
    );
  }

  const date = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  return (
    <div className={styles.results}>
      {!layouts ? (
        <div className={styles.resultsEmpty}>{error ?? <Spinner size={14} />}</div>
      ) : layouts.length === 0 ? (
        <div className={styles.resultsEmpty}>No chart layouts are saved on your TradingView account.</div>
      ) : (
        layouts.map((l) => (
          <button key={l.id} type="button" className={styles.indicatorEntry} onClick={() => setPicked(l)}>
            <span className={styles.indicatorEntryName}>{l.name}</span>
            <span className={styles.resultMeta}>
              {l.symbol} · {l.interval}
            </span>
            <span className={styles.resultMeta}>{l.modified ? date.format(l.modified * 1000) : ''}</span>
          </button>
        ))
      )}
    </div>
  );
}
