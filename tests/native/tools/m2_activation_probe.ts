import { config } from './config.ts';

// Qualification-only activation telemetry. The positive timing module is separate.
export const register = (on: any) => {
  on('session.start', ($: any, e: any, next: any) => {
    const forwarded = next(e);
    void (async () => {
      let phase = 'entry';
      const identity = { kind: 'ACTIVATION_ONLY', runId: config.runId, nonce: config.nonce,
        activationSha256: config.activationSha256, positiveProbeSha256: config.positiveProbeSha256 };
      try {
        await $.fs.write(config.entryOutput, JSON.stringify({ ...identity, stage: 'CALLBACK_ENTERED' }));
        phase = 'forward';
        await forwarded;
        phase = 'trace';
        const trace = next.trace.map((link: any) => ({ index: link.index, plugin: link.plugin,
          tier: link.tier, event: link.event, outcome: link.outcome, reason: link.reason }));
        const origin = { plugin: next.origin.plugin, tier: next.origin.tier };
        phase = 'forward-output';
        await $.fs.write(config.forwardOutput, JSON.stringify({ ...identity, stage: 'DOWNSTREAM_SETTLED',
          origin, event: next.event, trace }));
        phase = 'http';
        const route = ['ready', config.runId, config.responderId, config.nonce, config.activationSha256].join('/');
        const answer = await $.http.fetch('http://localhost/' + route,
          { socketPath: config.socketPath, method: 'GET' });
        if (!answer.ok || answer.status !== 200) throw new Error('readiness rejected');
        const acknowledgement = JSON.parse(answer.text);
        for (const [key, value] of Object.entries(config.acknowledgement)) {
          if (acknowledgement[key] !== value) throw new Error('foreign readiness acknowledgement');
        }
        phase = 'result-output';
        await $.fs.write(config.resultOutput, JSON.stringify({ ...identity, stage: 'READINESS_READ_BACK', acknowledgement }));
      } catch (error) {
        const failure = phase === 'forward' ? 'DOWNSTREAM_FAILURE' : phase === 'trace' ? 'CALLBACK_FAILURE' : phase === 'http' ? 'HTTP_READINESS_FAILURE' : 'OUTPUT_WRITE_FAILURE';
        try {
          await $.fs.write(config.errorOutput, JSON.stringify({ ...identity, stage: failure, phase,
            errorType: error instanceof Error ? error.name : 'unknown' }));
        } catch { /* Host diagnostics and controller readback remain independent. */ }
      }
    })();
    return forwarded;
  });
};
