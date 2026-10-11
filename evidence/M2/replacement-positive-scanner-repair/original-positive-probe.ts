import { createLatencyMeasurement } from './latency.ts';
import { startBackground } from './background.ts';
import { config } from './config.ts';

export const register = (on: any) => {
  const meter = createLatencyMeasurement(['/unused-capture-helper', 'mod-batch', '--qualification-latency'], config.runtimeId)!;
  on('session.start', ($: any, e: any, next: any) => {
    const forwarded = next(e);
    void (async () => {
      const rows: any[] = [];
      const responses: any[] = [];
      const background = config.condition === 'background' ? startBackground($) : null;
      let sequence = config.firstSequence;
      const read = () => {
        const result = meter.readCapture();
        if (result === null) throw new Error('collector clock read failed');
        return result;
      };
      const marker = async () => {
        const entry = config.plan[sequence];
        const path = [config.runId, config.bootId, config.responderId, config.runtimeId,
          config.sourceEpoch, sequence, entry.challenge].join('/');
        const answer = await $.http.fetch('http://localhost/' + path, { socketPath: config.socketPath, method: 'GET' });
        if (!answer.ok || answer.status !== 200) throw new Error('native responder rejected marker');
        const response = JSON.parse(answer.text);
        for (const [key, value] of Object.entries({ ...entry, runId: config.runId, bootId: config.bootId,
          responderId: config.responderId, manifestSha256: config.freezeSha256, clock: 'CLOCK_UPTIME_RAW' })) {
          if (response[key] !== value) throw new Error('foreign or reordered native response: ' + key);
        }
        responses.push(response);
        sequence++;
        return response;
      };
      const publish = (complete: boolean, error: string | null, fixture: any = null) => $.fs.write(config.output,
        JSON.stringify({ runtimeId: config.runtimeId, sourceEpoch: config.sourceEpoch, condition: config.condition,
          rows, responses, complete, error, background: fixture, wrapper: performance.now.toString() }));
      try {
        for (const delay of config.delays) {
          const readings: number[] = [];
          for (let i = 0; i < 256; i++) readings.push(read());
          const row: any = { requestedDelayMs: delay, condition: config.condition, runtimeId: config.runtimeId,
            sourceEpoch: config.sourceEpoch, readings };
          rows.push(row);
          row.captureLower = await marker();
          readings.push(read());
          row.captureUpper = await marker();
          await new Promise<void>(resolve => $.clock.after(delay, resolve));
          row.endLower = await marker();
          readings.push(read());
          row.endUpper = await marker();
          await publish(false, null);
        }
        await publish(true, null, background ? await background : null);
      } catch (error) {
        await publish(false, String(error));
      }
    })();
    return forwarded;
  });
};
