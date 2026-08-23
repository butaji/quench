// Node compat: diagnostics_channel channels + tracingChannel + boundedChannel.
const dc = require('node:diagnostics_channel');
if (typeof dc.channel !== 'function') throw new Error('no channel fn');
if (typeof dc.tracingChannel !== 'function') throw new Error('no tracingChannel fn');
if (typeof dc.boundedChannel !== 'function') throw new Error('no boundedChannel fn');

// Channel subscribe/publish/unsubscribe.
const ch = dc.channel('quench-fixture');
let got = null;
const fn = (m) => { got = m.v; };
ch.subscribe(fn);
ch.publish({ v: 7 });
if (got !== 7) throw new Error('publish=' + got);
if (!ch.hasSubscribers) throw new Error('hasSubscribers');
ch.unsubscribe(fn);
if (ch.hasSubscribers) throw new Error('unsub');
ch.publish({ v: 9 });
if (got !== 7) throw new Error('leak');
if (!dc.channelNames().some((n) => n === 'quench-fixture')) throw new Error('channelNames');

// tracingChannel traceSync lifecycle: start -> fn -> end.
const trace = dc.tracingChannel('quench-trace');
const events = [];
trace.start.subscribe((c) => events.push('start:' + c.value));
trace.end.subscribe((c) => events.push(c.result === 5 ? 'end:5' : 'end'));
trace.error.subscribe((c) => events.push('error:' + c.error.message));
const syncResult = trace.traceSync((value) => value + 1, { value: 4 }, undefined, 4);
if (syncResult !== 5) throw new Error('traceSync result=' + syncResult);
if (events.join('|') !== 'start:4|end:5') throw new Error('traceSync events=' + events.join('|'));

// tracingChannel error propagation.
events.length = 0;
let threw = false;
try {
  trace.traceSync(() => { throw new Error('boom'); }, { value: 1 }, undefined);
} catch (e) { threw = e.message === 'boom'; }
if (!threw) throw new Error('traceSync did not rethrow');
if (events.join('|') !== 'start:1|error:boom|end') throw new Error('traceSync error events=' + events.join('|'));

// tracingChannel tracePromise async lifecycle.
events.length = 0;
trace.asyncStart.subscribe((c) => events.push('asyncStart:' + c.result));
trace.asyncEnd.subscribe((c) => events.push('asyncEnd:' + c.result));
Promise.resolve(trace.tracePromise(() => Promise.resolve(8), { value: 8 }, undefined))
  .then((value) => {
    if (value !== 8) throw new Error('tracePromise result=' + value);
    if (events.join('|') !== 'start:8|end|asyncStart:8|asyncEnd:8') {
      throw new Error('tracePromise events=' + events.join('|'));
    }
    // boundedChannel surface.
    const bc = dc.boundedChannel('bounded');
    const bcEvents = [];
    bc.start.subscribe(() => bcEvents.push('start'));
    bc.end.subscribe(() => bcEvents.push('end'));
    if (!bc.hasSubscribers) throw new Error('bounded hasSubscribers');
    console.log('diagnostics_channel: ok');
  })
  .catch((e) => { console.error('diagnostics_channel FAIL', e.message); process.exitCode = 1; });