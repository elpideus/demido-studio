// The Browser window: tabs, an address bar, and the page. The page is not drawn by this
// webview: each tab is a native webview of its own that the backend lays over the spot this
// window leaves for it (`api.browserPlace`). Nothing the app draws can show above a native view,
// so while a menu, a toast or another window covers that spot the page is put out of sight and
// the spot shows what the page is instead.

import { type FormEvent, type KeyboardEvent, type RefObject, useEffect, useRef, useState } from 'react';
import { ArrowLeft, ArrowRight, Bot, Globe, Hand, Plus, RotateCw, Search, X } from 'lucide-react';
import { Button, IconButton, cx } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { BrowserRect, BrowserTab } from '@/lib/types';
import { useBrowser } from '@/stores/browser';
import { useChats } from '@/stores/chats';
import { toast } from '@/stores/toasts';
import { addressOf, countdown, coverPoints, insideHandles, isBlank, sameRect, shownTab, tabLabel } from './browserView';
import styles from './BrowserWindow.module.css';

/** How often the page's spot is checked for something drawn over it. */
const COVER_EVERY_MS = 120;

function failed(what: string) {
  return (e: unknown) => toast.error(what, errorText(e));
}

const asRect = (r: DOMRect): BrowserRect => ({ x: r.left, y: r.top, width: r.width, height: r.height });

/** Where the page goes in `el`: all of it but the window's resize handles. */
function pageRect(el: HTMLElement): BrowserRect {
  const frame = el.closest('[data-window]');
  const handles = [...(frame?.querySelectorAll<HTMLElement>('[data-resize-handle]') ?? [])].map((h) => ({
    edge: h.dataset.resizeHandle ?? '',
    at: asRect(h.getBoundingClientRect()),
  }));
  return insideHandles(asRect(el.getBoundingClientRect()), handles);
}

/**
 * Whether anything the app draws is over `rect` of `el`. What belongs to the Browser window
 * itself (its corners' resize handles) does not count.
 */
function coveredOver(el: HTMLElement, rect: BrowserRect): boolean {
  const frame = el.closest('[data-window]') ?? el;
  for (const [x, y] of coverPoints(rect)) {
    const hit = document.elementFromPoint(x, y);
    // Off the app's window: nothing of the app's can be there.
    if (!hit) continue;
    if (!frame.contains(hit)) return true;
  }
  return false;
}

/**
 * Keeps the page's native view on `spot`, or out of sight when `show` is false or something
 * covers the spot. Returns whether something covers it.
 */
function usePlacement(spot: RefObject<HTMLDivElement | null>, show: boolean): boolean {
  const [covered, setCovered] = useState(false);
  useEffect(() => {
    let frame = 0;
    let sent: BrowserRect | null | undefined;
    let checked = -Infinity;
    let over = false;
    const send = (rect: BrowserRect | null) => {
      if (sent !== undefined && sameRect(rect, sent)) return;
      sent = rect;
      void api.browserPlace(rect).catch(() => {});
    };
    const tick = () => {
      const el = spot.current;
      if (!show || !el) {
        send(null);
        return;
      }
      const rect = pageRect(el);
      if (rect.width < 2 || rect.height < 2) {
        send(null);
        return;
      }
      const now = performance.now();
      if (now - checked >= COVER_EVERY_MS) {
        checked = now;
        const nowOver = coveredOver(el, rect);
        if (nowOver !== over) {
          over = nowOver;
          setCovered(nowOver);
        }
      }
      send(over ? null : rect);
    };
    // Every frame, so the page follows a window being dragged; and on a timer too, since frames
    // stop when the webview thinks nothing on it changes.
    const loop = () => {
      tick();
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
    const timer = window.setInterval(tick, COVER_EVERY_MS);
    tick();
    return () => {
      cancelAnimationFrame(frame);
      window.clearInterval(timer);
      void api.browserPlace(null).catch(() => {});
    };
  }, [spot, show]);
  return show && covered;
}

function TabButton({ tab, chatTitle }: { tab: BrowserTab; chatTitle: string | null }) {
  const label = tabLabel(tab);
  const owner = tab.owner.kind === 'chat' ? `Used by the assistant in “${chatTitle ?? 'a chat'}”` : null;
  return (
    <div className={cx(styles.tab, tab.active && styles.tabActive)}>
      {/* A plain title, not the app's tooltip: that would be drawn under the page. */}
      <button
        type="button"
        className={styles.tabMain}
        title={[label, addressOf(tab), owner].filter(Boolean).join('\n')}
        onClick={() => void api.browserActivate(tab.id)}
      >
        {tab.owner.kind === 'chat' ? (
          <Bot size={13} strokeWidth={1.9} className={styles.tabBot} aria-label="The assistant's tab" />
        ) : (
          <Globe size={13} strokeWidth={1.8} className={styles.tabIcon} aria-hidden />
        )}
        <span className={styles.tabTitle}>{label}</span>
        {tab.loading && <span className={styles.tabLoading} aria-label="Loading" />}
      </button>
      <button
        type="button"
        className={styles.tabClose}
        aria-label={`Close ${label}`}
        title="Close tab"
        onClick={() => void api.browserCloseTab(tab.id)}
      >
        <X size={12} strokeWidth={2} aria-hidden />
      </button>
    </div>
  );
}

/** The assistant waits for the person: a check to pass, a sign-in, a choice to make. */
function AttentionBanner({ shown }: { shown: BrowserTab | undefined }) {
  const attention = useBrowser((s) => s.attention);
  const chat = useChats((s) => s.chats.find((c) => c.id === attention?.chatId));
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!attention) return undefined;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [attention]);
  if (!attention) return null;
  const here = shown?.id === attention.tab;
  const answer = (done: boolean) => void api.browserAnswer(done).catch(failed('Could not answer'));
  return (
    <div className={styles.attention} role="status">
      <Hand size={16} strokeWidth={1.9} className={styles.attentionIcon} aria-hidden />
      <div className={styles.attentionText}>
        <div className={styles.attentionTitle}>
          The assistant needs you{chat ? ` · ${chat.title}` : ''}
          <span className={styles.attentionTime}>{countdown(attention.until, now)}</span>
        </div>
        <div className={styles.attentionMessage}>{attention.message}</div>
      </div>
      {!here && (
        <Button size="sm" variant="secondary" onClick={() => void api.browserActivate(attention.tab)}>
          Show the tab
        </Button>
      )}
      <Button size="sm" variant="ghost" onClick={() => answer(false)}>
        Skip
      </Button>
      <Button size="sm" variant="primary" onClick={() => answer(true)}>
        Done
      </Button>
    </div>
  );
}

/** What a blank tab, or the window without tabs, shows: somewhere to type. */
function StartPage({ onGo }: { onGo: (input: string) => void }) {
  const [text, setText] = useState('');
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (text.trim()) onGo(text.trim());
  };
  return (
    <div className={styles.start}>
      <Globe size={40} strokeWidth={1.3} className={styles.startIcon} aria-hidden />
      <form className={styles.startForm} onSubmit={submit}>
        <Search size={16} className={styles.startSearch} aria-hidden />
        <input
          className={styles.startInput}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Search the web or type an address"
          spellCheck={false}
          autoFocus
        />
      </form>
      <p className={styles.startNote}>
        The assistant reads pages here when a site needs a real browser, each chat in its own tab (marked with a robot).
        When a page asks for a person, such as a captcha or a sign-in, it shows you the tab and waits.
      </p>
    </div>
  );
}

/** Where the page goes, and what shows there while the page cannot. */
function PageSpot({ tab, covered }: { tab: BrowserTab; covered: boolean }) {
  return (
    <div className={cx(styles.hidden, covered && styles.hiddenShown)}>
      <Globe size={28} strokeWidth={1.3} aria-hidden />
      <div className={styles.hiddenTitle}>{tabLabel(tab)}</div>
      <div className={styles.hiddenNote}>The page is hidden while something is over it.</div>
    </div>
  );
}

export function BrowserWindow() {
  const tabs = useBrowser((s) => s.tabs);
  const chats = useChats((s) => s.chats);
  const shown = shownTab(tabs);
  const blank = !shown || isBlank(shown.url);
  const spot = useRef<HTMLDivElement>(null);
  const covered = usePlacement(spot, !blank);
  const address = useRef<HTMLInputElement>(null);
  const [typed, setTyped] = useState<string | null>(null);

  // A new page, or another tab, shows its own address over what was being typed.
  useEffect(() => setTyped(null), [shown?.id, shown?.url]);

  const go = (input: string) => {
    if (shown) void api.browserNavigate(shown.id, input).catch(failed('Could not open the page'));
    else void api.browserNewTab(input).catch(failed('Could not open the page'));
  };
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const input = (typed ?? '').trim();
    if (!input) return;
    go(input);
    setTyped(null);
    address.current?.blur();
  };
  const history = (action: 'back' | 'forward' | 'stop' | 'reload') => {
    if (shown) void api.browserHistory(shown.id, action).catch(failed('The page did not answer'));
  };
  const newTab = () => void api.browserNewTab().catch(failed('Could not open a tab'));

  const onKey = (e: KeyboardEvent) => {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    const key = e.key.toLowerCase();
    if (key === 'l') {
      e.preventDefault();
      address.current?.focus();
      address.current?.select();
    } else if (key === 't') {
      e.preventDefault();
      newTab();
    } else if (key === 'r') {
      e.preventDefault();
      history('reload');
    }
  };

  const titleOf = (tab: BrowserTab) => {
    const owner = tab.owner;
    return owner.kind === 'chat' ? (chats.find((c) => c.id === owner.chatId)?.title ?? null) : null;
  };

  return (
    <div className={styles.window} onKeyDown={onKey}>
      <div className={styles.tabs} role="tablist" aria-label="Tabs">
        {tabs.map((t) => (
          <TabButton key={t.id} tab={t} chatTitle={titleOf(t)} />
        ))}
        <IconButton icon={Plus} label="New tab" size="sm" tooltip={false} title="New tab (Ctrl+T)" onClick={newTab} />
      </div>
      <form className={styles.toolbar} onSubmit={submit}>
        <IconButton
          icon={ArrowLeft}
          label="Back"
          size="sm"
          tooltip={false}
          title="Back"
          disabled={blank}
          onClick={() => history('back')}
        />
        <IconButton
          icon={ArrowRight}
          label="Forward"
          size="sm"
          tooltip={false}
          title="Forward"
          disabled={blank}
          onClick={() => history('forward')}
        />
        {shown?.loading ? (
          <IconButton icon={X} label="Stop" size="sm" tooltip={false} title="Stop" onClick={() => history('stop')} />
        ) : (
          <IconButton
            icon={RotateCw}
            label="Reload"
            size="sm"
            tooltip={false}
            title="Reload (Ctrl+R)"
            disabled={blank}
            onClick={() => history('reload')}
          />
        )}
        <input
          ref={address}
          className={styles.address}
          value={typed ?? addressOf(shown)}
          onChange={(e) => setTyped(e.target.value)}
          onFocus={(e) => e.target.select()}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              setTyped(null);
              e.currentTarget.blur();
            }
          }}
          placeholder="Search the web or type an address"
          aria-label="Address"
          spellCheck={false}
        />
      </form>
      <AttentionBanner shown={shown} />
      {blank ? (
        <StartPage key={shown?.id ?? 'none'} onGo={go} />
      ) : (
        <div ref={spot} className={styles.spot}>
          <PageSpot tab={shown} covered={covered} />
        </div>
      )}
    </div>
  );
}
