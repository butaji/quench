// Node compat: events.removeListener / removeAllListeners / once /
// prependListener / eventNames (all Bun-green).
const { EventEmitter } = require('node:events');
const ee = new EventEmitter();

let count = 0;
const h = () => { count += 1; };
ee.on('x', h);
ee.removeListener('x', h);
ee.emit('x');
if (count !== 0) throw new Error('removeListener failed: ' + count);

// once: fires once.
let onceCount = 0;
ee.once('y', () => { onceCount += 1; });
ee.emit('y');
ee.emit('y');
if (onceCount !== 1) throw new Error('once fired more than once: ' + onceCount);

// prependListener: order respected.
const order = [];
ee.on('z', () => order.push('a'));
ee.prependListener('z', () => order.push('b'));
ee.emit('z');
if (order.join(',') !== 'b,a') throw new Error('prepend order: ' + order.join(','));

// removeAllListeners clears remaining listeners for that event.
ee.removeAllListeners('z');
order.length = 0;
ee.emit('z');
if (order.length !== 0) throw new Error('removeAllListeners failed');

// eventNames lists registered event names.
ee.on('alpha', () => {});
ee.on('beta', () => {});
const names = ee.eventNames().sort();
if (names.join(',') !== 'alpha,beta') throw new Error('eventNames: ' + names.join(','));

console.log('events-listeners: ok');