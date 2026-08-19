// Node compat: events.EventEmitter round-trip.
const { EventEmitter } = require('node:events');
const ee = new EventEmitter();
let count = 0;
ee.on('ping', () => { count += 1; });
ee.on('ping', () => { count += 10; });
const a = ee.emit('ping');
const b = ee.emit('nope');
if (!(count === 11)) throw new Error('count=' + count);
if (!(a === true)) throw new Error('emit-1=' + a);
if (!(b === false)) throw new Error('emit-2=' + b);
console.log('events: ' + count + ' ' + a + ' ' + b);
