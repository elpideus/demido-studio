import { CandlestickChart, CircleArrowDown, Mail, MessagesSquare, Settings, type LucideIcon } from 'lucide-react';
import { Logo, Tooltip, cx } from '@demido/ui';

import { updateButton } from '@/settings/updateView';
import { useApp } from '@/stores/app';
import { useChats } from '@/stores/chats';
import { useUpdates } from '@/stores/updates';
import { type WindowKind, useWindows } from '@/stores/windows';
import styles from './ActivityBar.module.css';

interface ItemProps {
  icon: LucideIcon;
  label: string;
  active: boolean;
  onClick: () => void;
  shortcut?: string;
}

function Item({ icon: Icon, label, active, onClick, shortcut }: ItemProps) {
  return (
    <Tooltip
      content={
        <span className={styles.tip}>
          {label}
          {shortcut && <span className={styles.tipKey}>{shortcut}</span>}
        </span>
      }
      placement="right"
      delay={250}
    >
      <button
        type="button"
        aria-label={label}
        aria-pressed={active}
        className={cx(styles.item, active && styles.active)}
        onClick={onClick}
      >
        <Icon size={22} strokeWidth={1.7} aria-hidden />
      </button>
    </Tooltip>
  );
}

/**
 * Shown while there is an update to get: a click installs a ready one (the app restarts) and
 * otherwise opens the Updates tab.
 */
function UpdateItem() {
  const status = useUpdates((s) => s.status);
  const restart = useUpdates((s) => s.restart);
  const button = updateButton(status);
  if (!button) return null;
  return (
    <Tooltip content={button.tooltip} placement="right" delay={250}>
      <button
        type="button"
        aria-label={button.tooltip}
        className={cx(styles.item, styles.update)}
        onClick={() => (button.restart ? restart() : useWindows.getState().open('settings', { tab: 'updates' }))}
      >
        <CircleArrowDown size={22} strokeWidth={1.7} aria-hidden />
      </button>
    </Tooltip>
  );
}

/** The fixed navigation rail on the left, like VS Code's activity bar. */
export function ActivityBar() {
  const chatListOpen = useApp((s) => s.settings?.chatListOpen ?? true);
  const patchSettings = useApp((s) => s.patchSettings);
  const windows = useWindows((s) => s.windows);
  const toggle = useWindows((s) => s.toggle);

  const isOpen = (kind: WindowKind) => windows.some((w) => w.kind === kind);

  return (
    <nav className={styles.bar} aria-label="Main">
      <Tooltip content="Home" placement="right" delay={250}>
        <button
          type="button"
          aria-label="Home"
          className={styles.brand}
          onClick={() => void useChats.getState().open(null)}
        >
          <Logo size={28} />
        </button>
      </Tooltip>
      <div className={styles.group}>
        <Item
          icon={MessagesSquare}
          label="Chats"
          shortcut="Ctrl+B"
          active={chatListOpen}
          onClick={() => void patchSettings({ chatListOpen: !chatListOpen })}
        />
        <Item icon={CandlestickChart} label="Market" active={isOpen('market')} onClick={() => toggle('market')} />
        <Item icon={Mail} label="Mail" active={isOpen('mail')} onClick={() => toggle('mail')} />
      </div>
      <div className={styles.spacer} />
      <div className={styles.group}>
        <UpdateItem />
        <Item
          icon={Settings}
          label="Settings"
          shortcut="Ctrl+,"
          active={isOpen('settings')}
          onClick={() => toggle('settings')}
        />
      </div>
    </nav>
  );
}
