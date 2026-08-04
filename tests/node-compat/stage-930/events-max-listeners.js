const assert = require("assert");
const events = require("events");

const emitter = new events.EventEmitter();
assert.strictEqual(events.getMaxListeners(emitter), events.defaultMaxListeners);
assert.strictEqual(events.setMaxListeners(101, emitter), emitter);
assert.strictEqual(events.getMaxListeners(emitter), 101);

const target = new EventTarget();
assert.strictEqual(events.getMaxListeners(target), events.defaultMaxListeners);
events.setMaxListeners(101, target);
assert.strictEqual(events.getMaxListeners(target), 101);
assert.strictEqual(events.getMaxListeners(new AbortController().signal), 0);
