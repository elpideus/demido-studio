// What a tool's download display draws. The backend sends `{kind: 'download', jobId, plan, status?,
// jobError?, note?}`; `status` is the job's status when the tool stopped waiting, and without a job
// `denied` / `done` say why nothing was started.

import type { MarketPlan, ToolDisplay } from '@/lib/types';

export type DownloadView =
  /** Live progress; `continuable` when the turn ended before the job finished. */
  | { kind: 'progress'; jobId: string | null; plan: MarketPlan | null; continuable: boolean }
  | { kind: 'note'; text: string }
  /** The download never started; only its error is shown. */
  | { kind: 'none' };

const str = (v: unknown) => (typeof v === 'string' ? v : '');

// A plan from an older backend has another shape; drawing it would throw inside the chat.
const planOf = (v: unknown) => (typeof (v as MarketPlan | null)?.requests === 'number' ? (v as MarketPlan) : null);

export function downloadView(d: ToolDisplay): DownloadView {
  const jobId = str(d.jobId) || null;
  const plan = planOf(d.plan);
  const status = str(d.status);
  const note = str(d.note);
  // A job the tool already saw finish was answered in the same turn; nothing to continue.
  if (jobId) return { kind: 'progress', jobId, plan, continuable: status !== 'done' };
  if (d.denied === true || status === 'denied') {
    return { kind: 'note', text: note || 'You chose not to download this.' };
  }
  if (status === 'done' || plan?.complete) {
    return { kind: 'note', text: note || `${plan?.name ?? 'This data'} is already downloaded.` };
  }
  if (str(d.jobError)) return { kind: 'none' };
  // No job and no reason given: DownloadProgress says so and offers to start the plan.
  return { kind: 'progress', jobId: null, plan, continuable: false };
}
