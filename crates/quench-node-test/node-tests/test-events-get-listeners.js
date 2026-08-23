// Node compat: events getEventListeners (already implemented).
const { EventEmitter, getEventListeners } = require('node:events');
if (typeof EventEmitter !== 'function') throw new Error('no EventEmitter');
if (typeof getEventListeners !== 'function') throw new Error('no getEventListeners');

const ee = new EventEmitter();
if (getEventListeners(ee, 'x').length !== 0) throw new Error('expected 0 initial listeners');

const a = function () {};
const b = function () {};
ee.on('x', a);
ee.on('x', b);
const arr = getEventListeners(ee, 'x');
if (arr.length !== 2) throw new Error('expected 2 listeners, got ' + arr.length);
if (arr[0] !== a || arr[1] !== b) throw new Error('listener identity mismatch');

ee.removeListener('x', a);
const after = getEventListeners(ee, 'x');
if (after.length !== 1 || after[0] !== b) throw new Error('after removeListener mismatch');

// Other event untouched
if (getEventListeners(ee, 'y').length !== 0) throw new Error('y should be empty');
ee.once('y', function () {});
const onceArr = getEventListeners(ee, 'y');
if (onceArr.length !== 1) throw new Error('once listener missing');

console.log('events-get-event-listeners: ok');