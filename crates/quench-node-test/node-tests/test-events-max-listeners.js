// Node compat: events max-listeners and listenerCount APIs (Bun-green).
const { EventEmitter, setMaxListeners, getMaxListeners, listenerCount } = require('node:events');

if (typeof setMaxListeners !== 'function') throw new Error('no setMaxListeners');
if (typeof getMaxListeners !== 'function') throw new Error('no getMaxListeners');
if (typeof listenerCount !== 'function') throw new Error('no listenerCount');

const ee = new EventEmitter();
if (ee.getMaxListeners() !== 10) throw new Error('default max=' + ee.getMaxListeners());
if (ee.listenerCount('x') !== 0) throw new Error('default count=' + ee.listenerCount('x'));

const a = function () {};
const b = function () {};
ee.on('x', a);
ee.on('x', b);
ee.on('y', a);
if (ee.listenerCount('x') !== 2) throw new Error('x count=' + ee.listenerCount('x'));
if (ee.listenerCount('y') !== 1) throw new Error('y count=' + ee.listenerCount('y'));
if (ee.listenerCount('z') !== 0) throw new Error('z count=' + ee.listenerCount('z'));

// setMaxListeners / getMaxListeners round-trip.
ee.setMaxListeners(2);
if (ee.getMaxListeners() !== 2) throw new Error('after set=' + ee.getMaxListeners());
ee.setMaxListeners(0);  // 0 means Infinity per Node semantics.
if (ee.getMaxListeners() !== 0) throw new Error('zero=' + ee.getMaxListeners());

console.log('events-max-listeners: ok');