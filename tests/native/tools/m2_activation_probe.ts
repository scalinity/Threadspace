import { config } from './config.ts';

// Exact 2.1.295 session.start shapes; private diagnostics, never timing samples.
const snapshot = (value: any, result: any) => {
  const names = result ? ['cwd'] : ['cwd', 'surface', 'isInteractive'];
  const fields: any = {};
  let supported = value !== null && typeof value === 'object' && !Array.isArray(value);
  const unexpected = supported ? Object.keys(value).filter((name: any) => !names.includes(name)) : [];
  if (unexpected.length) supported = false;
  for (const name of names) {
    if (value === null || typeof value !== 'object' || !Object.prototype.hasOwnProperty.call(value, name)) {
      fields[name] = { presence: 'absent' }; supported = false; continue;
    }
    const operand = value[name];
    const type = operand === null ? 'null' : typeof operand;
    fields[name] = { presence: 'present', type };
    if (['string', 'boolean', 'number'].includes(type)) fields[name].value = operand;
    if (name === 'cwd' ? type !== 'string' : name === 'isInteractive' ? type !== 'boolean' :
      !(type === 'null' || type === 'string' && ['terminal', 'desktop', 'mobile', 'vscode'].includes(operand))) supported = false;
  }
  return { schema: result ? 'SessionStartResult/2.1.295/v1' : 'SessionStartInput/2.1.295/v1', supported, fields, unexpected };
};

export const register = (on: any) => {
  on('session.start', ($: any, e: any, next: any) => {
    let original: any;
    try { original = snapshot(e, false); }
    catch { original = { supported: false, reason: 'snapshot-unavailable' }; }
    const forwarded = next(e);
    void (async () => {
      let phase = 'entry';
      const identity = { kind: 'ACTIVATION_READINESS_V1', runId: config.runId, nonce: config.nonce,
        activationSha256: config.activationSha256, positiveProbeSha256: config.positiveProbeSha256 };
      try {
        const origin = { plugin: next.origin.plugin, tier: next.origin.tier };
        const event = next.event;
        await $.fs.write(config.entryOutput, JSON.stringify({ ...identity, sequence: 1,
          stage: 'CALLBACK_ENTERED', origin, event, original }));
        phase = 'forward';
        const returned = await forwarded;
        phase = 'trace';
        const result = snapshot(returned, true);
        const trace = next.trace.map((link: any) => ({ index: link.index, plugin: link.plugin,
          tier: link.tier, event: link.event, outcome: link.outcome, reason: link.reason,
          received: snapshot(link.received, false), returned: snapshot(link.returned, true) }));
        phase = 'forward-output';
        await $.fs.write(config.forwardOutput, JSON.stringify({ ...identity, sequence: 2,
          stage: 'DOWNSTREAM_SETTLED', origin, event, result, trace }));
        phase = 'http';
        const route = ['ready', config.runId, config.responderId, config.nonce, config.activationSha256].join('/');
        const answer = await $.http.fetch('http://localhost/' + route, { socketPath: config.socketPath, method: 'GET' });
        if (!answer.ok || answer.status !== 200) throw new Error('readiness rejected');
        const acknowledgement = JSON.parse(answer.text);
        if (Object.keys(acknowledgement).length !== Object.keys(config.acknowledgement).length) throw new Error('foreign readiness acknowledgement');
        for (const [key, value] of Object.entries(config.acknowledgement)) {
          if (acknowledgement[key] !== value) throw new Error('foreign readiness acknowledgement');
        }
        phase = 'result-output';
        await $.fs.write(config.resultOutput, JSON.stringify({ ...identity, sequence: 3,
          stage: 'READINESS_READ_BACK', httpStatus: answer.status, acknowledgement }));
      } catch (error) {
        const failure = phase === 'forward' ? 'DOWNSTREAM_FAILURE' : phase === 'trace' ? 'CALLBACK_FAILURE' : phase === 'http' ? 'HTTP_READINESS_FAILURE' : 'OUTPUT_WRITE_FAILURE';
        try {
          await $.fs.write(config.errorOutput, JSON.stringify({ ...identity, stage: failure, phase,
            errorType: error instanceof Error ? error.name : 'unknown' }));
        } catch { /* Independent controller and fixture records remain authoritative. */ }
      }
    })();
    return forwarded;
  });
};
