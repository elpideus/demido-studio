// What the update page decides from how a run went. Kept apart from the page so it can be tested.

import type { StepView } from './Install';

/** What a finished run left in the install folder: the new version (the app step is done), the
 * previous one whole (the app step failed and put everything back, or never ran), or part of each
 * (the app step failed and could not undo all its changes; running setup again finishes it). */
export type Installed = 'new' | 'previous' | 'mixed';

export function installedAfter(steps: readonly Pick<StepView, 'id' | 'state' | 'incomplete'>[]): Installed {
  const app = steps.find((s) => s.id === 'app');
  if (app?.state === 'done') return 'new';
  if (app?.state === 'failed' && app.incomplete) return 'mixed';
  return 'previous';
}

/** Whether a run first checks that nothing runs from the folder, so the person can let setup close
 * it. The first run of an automatic update skips it: the app that started it is closing by itself,
 * and setup waits for it. A retry checks: whatever runs then will not close by itself. */
export function checksFirst(auto: boolean, retry: boolean): boolean {
  return !auto || retry;
}

export type FailureAction = 'retry' | 'open' | 'close';

/** The buttons on the page of an update that did not finish: the primary one, and the others. */
export function failureActions(
  auto: boolean,
  installed: Installed,
): { primary: FailureAction; others: FailureAction[] } {
  // Only another run makes the folder whole again, and the app it holds may not start.
  if (installed === 'mixed') return { primary: 'retry', others: ['close'] };
  // An automatic update closed the app the person was using: they get it back.
  return auto ? { primary: 'open', others: ['retry'] } : { primary: 'close', others: ['retry'] };
}
