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

console.log('EventEmitter prototype deletion: ok');
