import { describe, expect, it } from "vitest";

import { createInitBarrier, type GpuLike } from "./initBarrier";

function fakeGpu() {
  const calls: unknown[] = [];
  const adapter = { name: "adapter" };
  const gpu: GpuLike = {
    requestAdapter(options?: unknown) {
      calls.push(options);
      return Promise.resolve(adapter);
    },
  };
  return { gpu, calls, adapter, original: gpu.requestAdapter };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("renderer init barrier", () => {
  it("is never installed in a release build", () => {
    const { gpu, original } = fakeGpu();
    const barrier = createInitBarrier(gpu, false);
    expect(gpu.requestAdapter).toBe(original);
    expect(() => barrier.arm()).toThrow(/qualification builds/);
    expect(barrier.state().installed).toBe(false);
  });

  it("passes requests through until armed", async () => {
    const { gpu, adapter } = fakeGpu();
    createInitBarrier(gpu, true);
    await expect(gpu.requestAdapter()).resolves.toBe(adapter);
  });

  it("issues the armed request natively but withholds its result until release", async () => {
    const { gpu, calls, adapter } = fakeGpu();
    const barrier = createInitBarrier(gpu, true, () => 1000);
    barrier.arm();
    let delivered: unknown = null;
    void gpu.requestAdapter({ power: "high" }).then((value) => (delivered = value));
    await flush();
    expect(calls).toEqual([{ power: "high" }]);
    expect(delivered).toBeNull();
    expect(barrier.state().held).toEqual({ heldAtMs: 1000 });
    expect(barrier.state().armed).toBe(false);

    barrier.release();
    await flush();
    expect(delivered).toBe(adapter);
    expect(barrier.state().held).toBeNull();
    expect(barrier.state().releases).toEqual([{ heldAtMs: 1000, releasedAtMs: 1000, releasedBy: "COMMAND", adapterSettledWhileHeld: true }]);
    // One shot: the next request is not held.
    await expect(gpu.requestAdapter()).resolves.toBe(adapter);
  });

  it("refuses a release with nothing held", () => {
    const { gpu } = fakeGpu();
    expect(() => createInitBarrier(gpu, true).release()).toThrow(/no renderer init is held/);
  });
});
