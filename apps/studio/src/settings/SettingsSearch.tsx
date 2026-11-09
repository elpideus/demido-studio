import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { ChevronRight, CornerDownLeft, Search, X } from 'lucide-react';
import { Kbd, Popover, Tooltip, cx } from '@demido/ui';

import { useApp } from '@/stores/app';
import { useModels } from '@/stores/models';
import { useSkills } from '@/stores/skills';
import { useWindows } from '@/stores/windows';
import { PageTile } from './layout';
import { PAGES } from './pages';
import { highlight, searchSettings, settingEntries, type SettingEntry, type SettingTarget } from './searchIndex';
import s from './SettingsSearch.module.css';

interface Props {
  /** The Settings window, whose Ctrl+F this field answers while it is the focused window. */
  winId: string;
  /** The rail shows icons only: the field then opens from a button, in a popover. */
  collapsed: boolean;
  onPick: (target: SettingTarget) => void;
}

/**
 * "Search settings", on top of the Settings rail: finds a setting by its name or the words people
 * use for it, and a model or skill by name. Choosing a result opens its page and points at it.
 */
export function SettingsSearch({ winId, collapsed, onPick }: Props) {
  const models = useModels((st) => st.models);
  const skills = useSkills((st) => st.skills);
  const entries = useMemo(() => settingEntries(models, skills), [models, skills]);
  const shortcut = useApp((st) => st.info?.platform) === 'macos' ? '⌘F' : 'Ctrl+F';

  const [query, setQuery] = useState('');
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const results = useMemo(() => searchSettings(entries, query), [entries, query]);
  const input = useRef<HTMLInputElement>(null);
  const anchor = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const listId = useId();

  useEffect(() => setActive(0), [query]);
  useEffect(() => setOpen(false), [collapsed]);

  // Ctrl+F, while this Settings window is the one in front.
  const collapsedNow = useRef(collapsed);
  collapsedNow.current = collapsed;
  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return;
      if (event.key.toLowerCase() !== 'f' || useWindows.getState().focusedId !== winId) return;
      event.preventDefault();
      if (collapsedNow.current) {
        setOpen(true);
      } else {
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [winId]);

  const choose = (entry: SettingEntry | undefined) => {
    if (!entry) return;
    setOpen(false);
    onPick(entry.target);
  };

  // While results show, Escape goes to their popover, which clears the query; then it leaves the field.
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      setOpen(true);
      if (results.length === 0) return;
      const step = event.key === 'ArrowDown' ? 1 : results.length - 1;
      setActive((a) => (a + step) % results.length);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      choose(results[active]);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      if (query) setQuery('');
      else input.current?.blur();
    }
  };

  const field = (
    <div ref={anchor} className={cx(s.field, collapsed && s.inPopover)}>
      <Search className={s.searchIcon} size={14} strokeWidth={2} aria-hidden />
      <input
        ref={input}
        className={s.input}
        type="text"
        role="combobox"
        aria-label="Search settings"
        aria-expanded={open && query.trim() !== ''}
        aria-controls={listId}
        aria-activedescendant={open && results[active] ? `${listId}-${active}` : undefined}
        aria-autocomplete="list"
        placeholder="Search"
        spellCheck={false}
        autoComplete="off"
        autoFocus={collapsed}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => !collapsed && setOpen(false)}
        onKeyDown={onKeyDown}
      />
      {query ? (
        <button
          type="button"
          className={s.clear}
          aria-label="Clear search"
          // Keeps the focus in the field.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => {
            setQuery('');
            input.current?.focus();
          }}
        >
          <X size={12} strokeWidth={2.4} aria-hidden />
        </button>
      ) : (
        !collapsed && (
          <span className={s.shortcut} aria-hidden>
            <Kbd>{shortcut}</Kbd>
          </span>
        )
      )}
    </div>
  );

  const list = (
    <Results
      id={listId}
      query={query}
      results={results}
      active={active}
      onHover={setActive}
      onChoose={(i) => choose(results[i])}
    />
  );

  if (collapsed) {
    return (
      <>
        <Tooltip
          content={
            <span className={s.tip}>
              Search settings <span className={s.tipKey}>{shortcut}</span>
            </span>
          }
          placement="right"
          delay={250}
          disabled={open}
          className={s.anchor}
        >
          <button
            ref={button}
            type="button"
            className={cx(s.iconButton, open && s.iconButtonOpen)}
            aria-label="Search settings"
            aria-haspopup="dialog"
            aria-expanded={open}
            onClick={() => setOpen((o) => !o)}
          >
            <Search size={16} strokeWidth={1.8} aria-hidden />
          </button>
        </Tooltip>
        <Popover
          open={open}
          onClose={() => setOpen(false)}
          anchorRef={button}
          placement="right-start"
          width={360}
          aria-label="Search settings"
        >
          {field}
          {query.trim() ? list : <p className={s.hint}>Find a setting, a model or a skill.</p>}
        </Popover>
      </>
    );
  }

  return (
    <>
      {field}
      <Popover
        open={open && query.trim() !== ''}
        // Only Escape closes it this way; leaving the field closes it too.
        onClose={() => setQuery('')}
        dismissOnOutside={false}
        anchorRef={anchor}
        placement="bottom-start"
        width={340}
        aria-label="Search results"
      >
        {list}
      </Popover>
    </>
  );
}

function Results({
  id,
  query,
  results,
  active,
  onHover,
  onChoose,
}: {
  id: string;
  query: string;
  results: SettingEntry[];
  active: number;
  onHover: (i: number) => void;
  onChoose: (i: number) => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  if (results.length === 0) {
    return (
      <div className={s.empty}>
        <p className={s.emptyTitle}>No settings match “{query.trim()}”</p>
        <p className={s.emptyText}>Try another word, such as the name of a model or what the setting does.</p>
      </div>
    );
  }
  return (
    // Pressing a result keeps the focus in the field, so the keys still move through the list.
    <div onMouseDown={(e) => e.preventDefault()}>
      <div ref={list} id={id} role="listbox" aria-label="Settings" className={s.list}>
        {results.map((entry, i) => {
          const page = PAGES.find((p) => p.id === entry.target.tab)!;
          return (
            <div
              key={entry.id}
              id={`${id}-${i}`}
              role="option"
              aria-selected={i === active}
              data-index={i}
              className={cx(s.item, i === active && s.active)}
              onPointerMove={() => i !== active && onHover(i)}
              onClick={() => onChoose(i)}
            >
              <PageTile page={page} size={28} />
              <span className={s.text}>
                <span className={s.label}>
                  {highlight(entry.label, query).map((part, j) =>
                    part.match ? <mark key={j}>{part.text}</mark> : <span key={j}>{part.text}</span>,
                  )}
                </span>
                <span className={s.where}>
                  {page.label}
                  {entry.where && (
                    <>
                      <ChevronRight size={10} strokeWidth={2.4} aria-hidden />
                      <span className={s.whereDetail}>{entry.where}</span>
                    </>
                  )}
                </span>
              </span>
              {i === active && <CornerDownLeft className={s.enter} size={13} strokeWidth={2} aria-hidden />}
            </div>
          );
        })}
      </div>
      <div className={s.hints} aria-hidden>
        <span>
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd> to move
        </span>
        <span>
          <Kbd>↵</Kbd> to open
        </span>
        <span>
          <Kbd>Esc</Kbd> to close
        </span>
      </div>
    </div>
  );
}
