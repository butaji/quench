const assert = require('assert');
const EventEmitter = require('events');

assert.strictEqual(typeof EventEmitter.prototype.prependListener, 'function');
assert.strictEqual(delete EventEmitter.prototype.prependListener, true);
assert.strictEqual(EventEmitter.prototype.prependListener, undefined);

console.log('EventEmitter prototype deletion: ok');
