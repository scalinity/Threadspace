// Counts every animation frame scheduled through the target's
// `requestAnimationFrame` (SPEC §15.6: count internal scheduled Three work,
// not only application render callbacks). At three 0.186.1 the renderer's
// internal `Animation` loop looks `self.requestAnimationFrame` up on every
// frame, so wrapping the property on `window` before the first renderer is
// created observes that loop without touching any private renderer member.
// Qualification builds install it; release builds never wrap the global.

export interface FrameTarget {
  requestAnimationFrame(callback: FrameRequestCallback): number;
  cancelAnimationFrame(handle: number): void;
}

export interface FrameCounts {
  /** Frames requested since installation. */
  requested: number;
  /** Requested frames cancelled before they ran. */
  cancelled: number;
  /** Requested frames whose callback ran. */
  fired: number;
  /** Requested frames that have neither run nor been cancelled. A hidden page that merely throttles callbacks still shows them here. */
  pending: number;
}

export interface FrameCounter {
  counts(): FrameCounts;
}

const installed = new WeakMap<FrameTarget, FrameCounter>();

/** Wraps the target's frame scheduling once; later calls return the same counter. */
export function installFrameCounter(target: FrameTarget): FrameCounter {
  const existing = installed.get(target);
  if (existing) return existing;

  const request = target.requestAnimationFrame;
  const cancel = target.cancelAnimationFrame;
  const pending = new Set<number>();
  let requested = 0;
  let cancelled = 0;
  let fired = 0;

  target.requestAnimationFrame = (callback: FrameRequestCallback): number => {
    const handle: number = request.call(target, (time: number) => {
      if (pending.delete(handle)) fired += 1;
      callback(time);
    });
    requested += 1;
    pending.add(handle);
    return handle;
  };
  target.cancelAnimationFrame = (handle: number): void => {
    if (pending.delete(handle)) cancelled += 1;
    cancel.call(target, handle);
  };

  const counter: FrameCounter = {
    counts: () => ({ requested, cancelled, fired, pending: pending.size }),
  };
  installed.set(target, counter);
  return counter;
}
