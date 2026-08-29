const { on } = require('events');
const { NodeEventTarget } = require('internal/event_target');
const emitter = new NodeEventTarget();
const interval = setInterval(() => emitter.dispatchEvent(new Event('foo')), 0);
(async () => {
  let count = 0;
  for await (const [item] of on(emitter, 'foo')) {
    count++;
    if (item.type !== 'foo') throw new Error('bad event');
    if (count > 5) break;
  }
  clearInterval(interval);
  if (count !== 6) throw new Error(`count ${count}`);
})();
