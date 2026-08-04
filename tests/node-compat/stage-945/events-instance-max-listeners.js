const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
assert.strictEqual(emitter.getMaxListeners(), 10);
assert.strictEqual(emitter.setMaxListeners(3), emitter);
assert.strictEqual(emitter.getMaxListeners(), 3);
