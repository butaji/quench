const assert = require('assert');
const EventEmitter = require('events');

assert.strictEqual(typeof EventEmitter.prototype.prependListener, 'function');
assert.strictEqual(delete EventEmitter.prototype.prependListener, true);
assert.strictEqual(EventEmitter.prototype.prependListener, undefined);

assert.throws(
  () => new EventEmitter({ captureRejections: 1 }),
  { code: 'ERR_INVALID_ARG_TYPE', name: 'TypeError' }
);
assert.throws(
  () => new EventEmitter().on('foo', null),
  { code: 'ERR_INVALID_ARG_TYPE', name: 'TypeError' }
);

const captured = new EventEmitter({ captureRejections: true });
const error = new Error('captured');
let seen;
captured.on('event', () => ({ then(_resolve, reject) { reject(error); } }));
captured.on('error', (value) => { seen = value; });
captured.emit('event');
assert.strictEqual(seen, error);

assert.strictEqual(EventEmitter.captureRejections, false);
EventEmitter.captureRejections = true;
const inherited = new EventEmitter();
assert.strictEqual(inherited.captureRejections, undefined);
EventEmitter.captureRejections = false;
assert.strictEqual(typeof process.removeAllListeners, 'function');
process.removeAllListeners('unhandledRejection');

console.log('EventEmitter prototype deletion: ok');
