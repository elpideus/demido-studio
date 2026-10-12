import { afterEach, describe, expect, it, vi } from 'vitest';
import { afterFirstPaint } from './utils';

describe('afterFirstPaint', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('shows once the page has painted, and only once', () => {
    vi.useFakeTimers();
    let paint: FrameRequestCallback | undefined;
    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
      paint = cb;
      return 1;
    });
    const show = vi.fn();
    afterFirstPaint(show);
    expect(show).not.toHaveBeenCalled();
    paint?.(0);
    expect(show).toHaveBeenCalledTimes(1);
    vi.runAllTimers();
    expect(show).toHaveBeenCalledTimes(1);
  });

  it('does not wait long for a paint a hidden window never gets', () => {
    vi.useFakeTimers();
    vi.stubGlobal('requestAnimationFrame', () => 1);
    const show = vi.fn();
    afterFirstPaint(show, 200);
    vi.advanceTimersByTime(199);
    expect(show).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(show).toHaveBeenCalledTimes(1);
  });
});
