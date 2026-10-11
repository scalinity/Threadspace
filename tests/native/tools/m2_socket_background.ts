// Frozen qualification fixture: local integer work and documented clock timers.
export const startBackground = ($: any) => {
  let ticks = 0, operations = 0, x = 1;
  const wait = (ms: number) => new Promise<void>(resolve => $.clock.after(ms, resolve));
  const done = (async () => {
    for (let tick = 0; tick < 256; tick++) {
      await wait(5);
      for (let i = 0; i < 2048; i++) {
        x = ((x ^ i) * 1664525 + 1013904223) >>> 0;
        operations++;
      }
      ticks++;
    }
    return { ticks, operations, checksum: x, complete: true };
  })();
  return done;
};
