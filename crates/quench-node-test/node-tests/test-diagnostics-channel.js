// Node compat: node:diagnostics_channel channel subscribe/publish/unsubscribe.
const dc = require('node:diagnostics_channel');
if (typeof dc.channel !== 'function') throw new Error('no channel fn');
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
if (!Array.isArray(dc.channelNames()) || dc.channelNames().indexOf('quench-fixture') < 0) {
  throw new Error('channelNames');
}
console.log('diagnostics_channel: ok');