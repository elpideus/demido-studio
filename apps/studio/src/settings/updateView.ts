// What the Updates tab's status card, the activity bar's update button and the "Update ready"
// toast say for an updater status. Pure, so the wording of every phase is tested in one place.

import { formatBytes } from '@demido/ui';

import type { UpdateStatus } from '@/lib/types';

/** What the status card's button does. */
export type UpdateAction =
  /** Check for updates (also "Try again" after an error). */
  | 'check'
  /** Download, then install. */
  | 'update'
  /** Install the ready update: the app restarts. */
  | 'restart'
  | 'cancel'
  /** Open the release page: this copy cannot install updates itself. */
  | 'github';

export type CardIcon = 'ok' | 'idle' | 'busy' | 'available' | 'download' | 'ready' | 'error';

export interface StatusCard {
  icon: CardIcon;
  title: string;
  meta: string | null;
  /** A "Release notes" link after the meta. */
  notesUrl: string | null;
  /** Download progress, 0 to 1 (null while the size is unknown); undefined when not downloading. */
  progress?: number | null;
  action: { kind: UpdateAction; label: string; primary: boolean } | null;
  /** The last attempt to install did not finish. */
  warning: string | null;
  /** Windows will ask for permission before installing. */
  adminNote: string | null;
}

export const ADMIN_NOTE = 'Windows will ask for administrator permission.';

const DAY_FORMAT: Intl.DateTimeFormatOptions = { year: 'numeric', month: 'short', day: 'numeric' };

export function formatDay(ms: number): string {
  return new Date(ms).toLocaleDateString([], DAY_FORMAT);
}

/** `just now`, `5 minutes ago`, `yesterday`, … and past a month the date. */
export function relativeTime(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return '';
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 45) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return minutes === 1 ? 'a minute ago' : `${minutes} minutes ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return hours === 1 ? 'an hour ago' : `${hours} hours ago`;
  const days = Math.round(hours / 24);
  if (days === 1) return 'yesterday';
  if (days < 30) return `${days} days ago`;
  return `on ${formatDay(then)}`;
}

function versionName(status: UpdateStatus): string {
  return status.release ? `Version ${status.release.version}` : 'A new version';
}

/** `Pre-release · Released Sep 20, 2026`, or whichever part is known. */
function releaseMeta(status: UpdateStatus): string | null {
  const release = status.release;
  if (!release) return null;
  const published = release.publishedAt ? Date.parse(release.publishedAt) : NaN;
  const parts = [
    release.prerelease ? 'Pre-release' : null,
    Number.isFinite(published) ? `Released ${formatDay(published)}` : null,
  ].filter((p): p is string => p !== null);
  return parts.length ? parts.join(' · ') : null;
}

function downloadMeta(status: UpdateStatus): { meta: string; progress: number | null } {
  const p = status.progress;
  const total = p?.total ?? status.release?.size ?? null;
  const done = p?.downloaded ?? 0;
  const amount = total ? `${formatBytes(done)} of ${formatBytes(total)}` : formatBytes(done);
  const speed = p && p.bytesPerSecond > 0 ? ` · ${formatBytes(p.bytesPerSecond)}/s` : '';
  return { meta: amount + speed, progress: total ? Math.min(1, done / total) : null };
}

const GITHUB = { kind: 'github', label: 'Download from GitHub', primary: false } as const;

export function statusCard(status: UpdateStatus, now = Date.now()): StatusCard {
  const base = { notesUrl: null, warning: null, adminNote: null };
  const canInstall = !status.unsupported;
  const adminNote = canInstall && status.needsAdmin ? ADMIN_NOTE : null;
  const version = status.release?.version ?? '';
  switch (status.phase) {
    case 'checking':
      return { ...base, icon: 'busy', title: 'Checking for updates…', meta: null, action: null };
    case 'upToDate':
      return {
        ...base,
        icon: 'ok',
        title: 'Demido Studio is up to date',
        meta: status.lastChecked
          ? `Version ${status.currentVersion} · checked ${relativeTime(status.lastChecked, now)}`
          : `Version ${status.currentVersion}`,
        action: { kind: 'check', label: 'Check for updates', primary: false },
      };
    case 'available':
      return {
        ...base,
        icon: 'available',
        title: `${versionName(status)} is available`,
        meta: releaseMeta(status),
        notesUrl: status.release?.url ?? null,
        action: canInstall ? { kind: 'update', label: 'Update', primary: true } : status.release ? GITHUB : null,
        adminNote,
      };
    case 'downloading': {
      const { meta, progress } = downloadMeta(status);
      return {
        ...base,
        icon: 'download',
        title: version ? `Downloading ${version}` : 'Downloading the update',
        meta,
        progress,
        action: { kind: 'cancel', label: 'Cancel', primary: false },
      };
    }
    case 'ready':
      if (!canInstall) {
        return {
          ...base,
          icon: 'available',
          title: `${versionName(status)} is available`,
          meta: releaseMeta(status),
          notesUrl: status.release?.url ?? null,
          action: status.release ? GITHUB : null,
        };
      }
      return {
        ...base,
        icon: 'ready',
        title: `${versionName(status)} is ready to install`,
        meta: 'Demido Studio restarts to finish. It takes a few seconds.',
        action: { kind: 'restart', label: status.failedAttempt ? 'Try again' : 'Restart and update', primary: true },
        warning: status.failedAttempt ? `The last attempt to install ${version || 'it'} did not finish.` : null,
        adminNote,
      };
    case 'installing':
      return { ...base, icon: 'busy', title: 'Restarting to update…', meta: null, action: null };
    case 'error':
      return {
        ...base,
        icon: 'error',
        title: 'Could not update',
        meta: status.error,
        action: { kind: 'check', label: 'Try again', primary: false },
      };
    case 'idle':
    default:
      return {
        ...base,
        icon: 'idle',
        title: `Version ${status.currentVersion}`,
        meta: 'Not checked yet',
        action: { kind: 'check', label: 'Check for updates', primary: false },
      };
  }
}

export interface UpdateButton {
  tooltip: string;
  /** Clicking installs now; otherwise it opens the Updates tab. */
  restart: boolean;
}

/** The activity bar's update button, or null when there is nothing to show. */
export function updateButton(status: UpdateStatus | null): UpdateButton | null {
  if (!status) return null;
  const version = status.release?.version ?? 'a new version';
  switch (status.phase) {
    case 'available':
      return { tooltip: `Update available: ${version}`, restart: false };
    case 'downloading': {
      const total = status.progress?.total ?? status.release?.size ?? 0;
      const done = status.progress?.downloaded ?? 0;
      const percent = total > 0 ? ` ${Math.min(100, Math.floor((done / total) * 100))}%` : '';
      return { tooltip: `Downloading update…${percent}`, restart: false };
    }
    case 'ready':
      return status.unsupported
        ? { tooltip: `Update available: ${version}`, restart: false }
        : { tooltip: `Restart to update to ${version}`, restart: true };
    case 'installing':
      return { tooltip: 'Restarting…', restart: false };
    default:
      return status.failedAttempt ? { tooltip: `Restart to update to ${version}`, restart: false } : null;
  }
}

/** What a new status means for an install the person asked for, and for the toasts. */
export interface StatusStep {
  /** The install the person asked for still stands. */
  installRequested: boolean;
  /** The download they asked to install is verified: restart now (asking first while a reply is being written). */
  restart: boolean;
  toast: { title: string; body: string } | null;
}

/**
 * Follows the store from `before` to `after`. "Update" only downloads; the backend never installs
 * by itself, so once that download is ready the UI restarts, and the question before a restart
 * that would stop a reply halfway is asked then, not at the click. The request lapses when the
 * update stops on the way (an error, a cancel that stopped the download).
 */
export function afterStatus(before: UpdateStatus | null, after: UpdateStatus, installRequested: boolean): StatusStep {
  const requested = installRequested && ['downloading', 'ready', 'installing'].includes(after.phase);
  const restart = requested && before?.phase === 'downloading' && after.phase === 'ready' && !after.unsupported;
  return { installRequested: requested, restart, toast: readyToast(before, after, requested) };
}

/**
 * The toast for an update that just finished downloading in automatic mode, or null. Only right
 * after the download (a check that ends back on "ready" is not news), and not when the person
 * asked to install it (the app restarts at once) or when this copy cannot install it.
 */
export function readyToast(
  before: UpdateStatus | null,
  after: UpdateStatus,
  installRequested: boolean,
): { title: string; body: string } | null {
  if (after.phase !== 'ready' || before?.phase !== 'downloading') return null;
  if (!after.auto || after.unsupported || installRequested) return null;
  const version = after.release?.version ?? 'The new version';
  return {
    title: 'Update ready',
    // A machine-wide installation is never installed at launch: it would ask for permission.
    body: after.needsAdmin
      ? `Demido Studio ${version} is ready to install. Windows will ask for permission.`
      : `Demido Studio ${version} installs the next time it starts.`,
  };
}
