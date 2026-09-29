import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  CircleAlert,
  CircleArrowDown,
  CircleCheck,
  Download,
  ExternalLink,
  Info,
  RefreshCw,
  RotateCw,
  ShieldCheck,
  TriangleAlert,
  type LucideIcon,
} from 'lucide-react';
import { Button, Field, Notice, ProgressBar, SegmentedControl, Spinner, Switch, cx } from '@demido/ui';

import { Markdown } from '@/chat/Markdown';
import type { UpdateChannel, UpdateStatus } from '@/lib/types';
import { useUpdates } from '@/stores/updates';
import { type CardIcon, type StatusCard, statusCard } from './updateView';
import s from './settings.module.css';
import styles from './UpdatesTab.module.css';

const CHANNELS: Array<{ value: UpdateChannel; label: string }> = [
  { value: 'release', label: 'Release' },
  { value: 'prerelease', label: 'Pre-release' },
];

const ICONS: Record<Exclude<CardIcon, 'busy'>, { icon: LucideIcon; tone: string | undefined }> = {
  ok: { icon: CircleCheck, tone: styles.accent },
  idle: { icon: RefreshCw, tone: undefined },
  available: { icon: CircleArrowDown, tone: styles.accent },
  download: { icon: Download, tone: styles.accent },
  ready: { icon: RotateCw, tone: styles.accent },
  error: { icon: CircleAlert, tone: styles.danger },
};

function StatusIcon({ icon }: { icon: CardIcon }) {
  if (icon === 'busy') {
    return (
      <span className={styles.icon}>
        <Spinner size={18} />
      </span>
    );
  }
  const { icon: Icon, tone } = ICONS[icon];
  return (
    <span className={cx(styles.icon, tone)}>
      <Icon size={20} strokeWidth={1.8} aria-hidden />
    </span>
  );
}

function openExternal(url: string) {
  if (/^https?:/i.test(url)) void openUrl(url);
}

function Action({ card, status }: { card: StatusCard; status: UpdateStatus }) {
  const check = useUpdates((st) => st.check);
  const update = useUpdates((st) => st.update);
  const restart = useUpdates((st) => st.restart);
  const cancel = useUpdates((st) => st.cancel);
  const action = card.action;
  if (!action) return null;
  const variant = action.primary ? 'primary' : 'secondary';
  switch (action.kind) {
    case 'check':
      return (
        <Button size="sm" variant={variant} icon={RefreshCw} onClick={() => void check()}>
          {action.label}
        </Button>
      );
    case 'update':
    case 'restart':
      // Both end in a restart, which asks first while a reply is still being written. "Update"
      // downloads first, so it asks when the download is done rather than now.
      return (
        <Button
          size="sm"
          variant={variant}
          icon={status.needsAdmin ? ShieldCheck : undefined}
          onClick={action.kind === 'update' ? update : restart}
        >
          {action.label}
        </Button>
      );
    case 'cancel':
      return (
        <Button size="sm" variant={variant} onClick={() => void cancel()}>
          {action.label}
        </Button>
      );
    case 'github':
      return (
        <Button
          size="sm"
          variant={variant}
          icon={ExternalLink}
          onClick={() => status.release && openExternal(status.release.url)}
        >
          {action.label}
        </Button>
      );
  }
}

/** Release notes, capped in height until "Show all". */
function ReleaseNotes({ notes }: { notes: string }) {
  const box = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  useLayoutEffect(() => {
    const el = box.current;
    if (el && !expanded) setOverflows(el.scrollHeight > el.clientHeight + 1);
  }, [notes, expanded]);
  return (
    <div className={cx(s.card, s.cardPad)}>
      <div ref={box} className={cx(styles.notes, !expanded && overflows && styles.capped, !expanded && styles.cap)}>
        <Markdown text={notes} />
      </div>
      {(overflows || expanded) && (
        <Button size="sm" variant="ghost" className={styles.more} onClick={() => setExpanded(!expanded)}>
          {expanded ? 'Show less' : 'Show all'}
        </Button>
      )}
    </div>
  );
}

export function UpdatesTab() {
  const status = useUpdates((st) => st.status);
  const setPreferences = useUpdates((st) => st.setPreferences);
  // Keeps "checked 5 minutes ago" current while the tab is open.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  if (!status) return null;
  const card = statusCard(status, now);
  const release = status.release;

  return (
    <div className={s.page}>
      <header className={s.header}>
        <div className={s.headerText}>
          <h2 className={s.title}>Updates</h2>
          <p className={s.subtitle}>Get new versions of Demido Studio as they come out.</p>
        </div>
      </header>
      <div className={s.scroll}>
        <section className={s.section}>
          <div className={s.stack}>
            {status.unsupported && (
              <Notice tone="info" icon={Info}>
                {status.unsupported}
              </Notice>
            )}
            {card.warning && (
              <Notice tone="warning" icon={TriangleAlert}>
                {card.warning}
              </Notice>
            )}
            <div className={s.rows}>
              <div className={cx(s.row, styles.status)} aria-live="polite">
                <StatusIcon icon={card.icon} />
                <div className={s.rowMain}>
                  <div className={s.rowTitle}>{card.title}</div>
                  {card.progress !== undefined && (
                    <ProgressBar value={card.progress} size="xs" className={styles.progress} label="Download" />
                  )}
                  {(card.meta || card.notesUrl) && (
                    <div className={cx(s.rowMeta, status.phase === 'error' && styles.wrap)}>
                      {card.meta}
                      {card.meta && card.notesUrl && ' · '}
                      {card.notesUrl && (
                        <a
                          href={card.notesUrl}
                          onClick={(e) => {
                            e.preventDefault();
                            openExternal(card.notesUrl!);
                          }}
                        >
                          Release notes
                        </a>
                      )}
                    </div>
                  )}
                  {card.adminNote && (
                    <div className={cx(s.rowMeta, styles.admin)}>
                      <ShieldCheck size={12} aria-hidden />
                      {card.adminNote}
                    </div>
                  )}
                </div>
                <div className={s.rowActions}>
                  <Action card={card} status={status} />
                </div>
              </div>
            </div>
          </div>
        </section>

        <section className={s.section}>
          <h3 className={s.sectionTitle}>Preferences</h3>
          <div className={cx(s.card, s.cardPad, styles.prefs)}>
            <Field
              layout="inline"
              label="Update channel"
              description="Pre-releases get new features first and may have rough edges."
            >
              <SegmentedControl
                size="sm"
                value={status.channel}
                onChange={(channel) => void setPreferences({ channel })}
                options={CHANNELS}
              />
            </Field>
            <Field
              layout="inline"
              label="Update automatically"
              description="Downloads new versions in the background and installs them the next time Demido Studio starts. Turn it off to check and update yourself."
            >
              <Switch
                checked={status.auto}
                onChange={(auto) => void setPreferences({ auto })}
                label="Update automatically"
              />
            </Field>
          </div>
        </section>

        {release && release.notes.trim() && (
          <section className={s.section}>
            <h3 className={s.sectionTitle}>What’s new in {release.version}</h3>
            <ReleaseNotes key={release.version} notes={release.notes} />
          </section>
        )}
      </div>
    </div>
  );
}
