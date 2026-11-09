import { useCallback, useEffect, useRef, type RefObject } from 'react';

import { type WindowState, useWindows } from '@/stores/windows';
import { TabbedLayout, type TabSpec } from '@/wm/TabbedLayout';
import { AboutTab } from './AboutTab';
import { ChatsTab } from './ChatsTab';
import { SettingsNavProvider } from './layout';
import { MailTab } from './MailTab';
import { ModelsTab } from './models/ModelsTab';
import { PAGES, pageFor, type SettingsTab } from './pages';
import { PermissionsTab } from './PermissionsTab';
import { ProvidersTab } from './ProvidersTab';
import { RuntimeTab } from './RuntimeTab';
import type { SettingTarget } from './searchIndex';
import { SettingsSearch } from './SettingsSearch';
import { SkillsTab } from './skills/SkillsTab';
import { UpdatesTab } from './UpdatesTab';
import { VoiceTab } from './VoiceTab';
import { WebTab } from './WebTab';
import styles from './SettingsWindow.module.css';

const TABS: Array<TabSpec<SettingsTab>> = PAGES.map(({ id, label, icon, tint, group }) => ({
  id,
  label,
  icon,
  tint,
  group,
}));

/**
 * Props: `tab` (the page), and on the way to a setting `focus` (the `data-setting` to point at),
 * `model` (the model whose editor is open), `skill` (the open skill) and `connect` (the provider
 * the Providers page starts connecting).
 */
export function SettingsWindow({ win }: { win: WindowState }) {
  const setProps = useWindows((s) => s.setProps);
  const root = useRef<HTMLDivElement>(null);
  const tab = pageFor(win.props.tab, 'models').id;

  const go = useCallback(
    (target: SettingTarget) =>
      setProps(win.id, {
        tab: target.tab,
        focus: target.focus,
        model: target.model,
        skill: target.skill,
        connect: target.connect,
      }),
    [setProps, win.id],
  );

  useReveal(root, typeof win.props.focus === 'string' ? win.props.focus : null, () =>
    setProps(win.id, { focus: undefined }),
  );

  return (
    <SettingsNavProvider value={go}>
      <div ref={root} className={styles.root}>
        <TabbedLayout
          tabs={TABS}
          active={tab}
          onChange={(id) => go({ tab: id })}
          header={(collapsed) => <SettingsSearch winId={win.id} collapsed={collapsed} onPick={go} />}
        >
          {tab === 'providers' && <ProvidersTab win={win} />}
          {tab === 'models' && <ModelsTab win={win} />}
          {tab === 'skills' && <SkillsTab win={win} />}
          {tab === 'chats' && <ChatsTab />}
          {tab === 'permissions' && <PermissionsTab />}
          {tab === 'voice' && <VoiceTab />}
          {tab === 'mail' && <MailTab />}
          {tab === 'web' && <WebTab />}
          {tab === 'runtime' && <RuntimeTab />}
          {tab === 'updates' && <UpdatesTab />}
          {tab === 'about' && <AboutTab />}
        </TabbedLayout>
      </div>
    </SettingsNavProvider>
  );
}

/** What can take the focus in a setting's row: its control. */
const CONTROL =
  'input:not([disabled]), textarea:not([disabled]), select:not([disabled]), button:not([disabled]):not([tabindex="-1"]), [tabindex]:not([tabindex="-1"])';

/**
 * Scrolls to the `[data-setting]` named `focus`, flashes it and puts the focus on its control,
 * then calls `done`. The page may still be loading what the setting sits in, so it looks again
 * every few milliseconds for a moment.
 */
function useReveal(root: RefObject<HTMLDivElement | null>, focus: string | null, done: () => void) {
  const finish = useRef(done);
  finish.current = done;
  useEffect(() => {
    if (!focus) return undefined;
    const started = performance.now();
    let timer = 0;
    const look = () => {
      const el = root.current?.querySelector<HTMLElement>(`[data-setting="${CSS.escape(focus)}"]`);
      if (!el) {
        if (performance.now() - started < 2000) timer = window.setTimeout(look, 30);
        else finish.current();
        return;
      }
      const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      el.scrollIntoView({ block: 'center', behavior: still ? 'auto' : 'smooth' });
      // Taken off and put back, so a second visit flashes again. A row of a group lights up within
      // it; anything else gets a ring around it.
      el.removeAttribute('data-flash');
      void el.offsetWidth;
      const row = el.hasAttribute('data-group') || !!el.parentElement?.closest('[data-group]');
      el.setAttribute('data-flash', row ? 'fill' : 'ring');
      window.setTimeout(() => el.removeAttribute('data-flash'), 1800);
      // One setting gets the focus on its control; a group is pointed at as a whole, unless it is
      // a set of choices, where the focus goes to the one chosen.
      const control = el.hasAttribute('data-group')
        ? el.querySelector<HTMLElement>('[role="radio"][aria-checked="true"]')
        : el.matches(CONTROL)
          ? el
          : (el.querySelector<HTMLElement>('[role="tablist"] [aria-selected="true"]') ??
            el.querySelector<HTMLElement>(CONTROL));
      control?.focus({ preventScroll: true });
      finish.current();
    };
    timer = window.setTimeout(look);
    return () => window.clearTimeout(timer);
  }, [root, focus]);
}
