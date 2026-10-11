let started = false;
export function register(on) {
  on('session.start', async ($, e, next) => {
    const result = next(e);
    if (!started) {
      started = true;
      try {
        const version = await $.session.version();
        await $.fs.write("__HARNESS_OWNED_GATE_OUTPUT__", JSON.stringify({complete:true, version, runtimeId:crypto.randomUUID(), wrapper:String(performance.now), oneReading:performance.now(), clockKeys:Object.keys(performance), callback:'session.start', nextCalls:1, inferenceRequested:false}));
      } catch (error) {
        await $.fs.write("__HARNESS_OWNED_GATE_OUTPUT__", JSON.stringify({complete:false, error:String(error?.message).slice(0,200), nextCalls:1, inferenceRequested:false}));
      }
    }
    return result;
  });
}
