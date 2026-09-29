// The update page's decisions after a run. Run with `pnpm --filter @demido/installer test`.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import type { StepId, StepState } from '../types';
import { checksFirst, failureActions, installedAfter } from './updateOutcome.ts';

const step = (id: StepId, state: StepState, incomplete?: boolean) => ({ id, state, incomplete });

describe('installedAfter', () => {
  it('is the new version once the app step is done, whatever failed after it', () => {
    assert.equal(installedAfter([step('app', 'done'), step('finalize', 'failed')]), 'new');
  });

  it('is the previous version when the app step put everything back', () => {
    assert.equal(installedAfter([step('app', 'failed')]), 'previous');
    assert.equal(installedAfter([step('app', 'failed', false)]), 'previous');
  });

  it('is the previous version when the app step never ran', () => {
    // A failure before the engine started (the app would not close) sends no plan at all.
    assert.equal(installedAfter([]), 'previous');
    assert.equal(installedAfter([step('app', 'pending'), step('runtime', 'pending')]), 'previous');
  });

  it('is part of each when the app step could not undo its changes', () => {
    assert.equal(installedAfter([step('app', 'failed', true), step('runtime', 'pending')]), 'mixed');
  });
});

describe('checksFirst', () => {
  it('skips the check only on the first run of an automatic update', () => {
    assert.equal(checksFirst(true, false), false);
    assert.equal(checksFirst(true, true), true, 'a retry offers to close what still runs');
    assert.equal(checksFirst(false, false), true);
    assert.equal(checksFirst(false, true), true);
  });
});

describe('failureActions', () => {
  it('never offers to open a folder holding part of each version', () => {
    for (const auto of [true, false]) {
      assert.deepEqual(failureActions(auto, 'mixed'), { primary: 'retry', others: ['close'] });
    }
  });

  it('gives the app back after an automatic update that kept the previous version', () => {
    assert.deepEqual(failureActions(true, 'previous'), { primary: 'open', others: ['retry'] });
  });

  it('closes setup after an update the person started', () => {
    assert.deepEqual(failureActions(false, 'previous'), { primary: 'close', others: ['retry'] });
  });
});
