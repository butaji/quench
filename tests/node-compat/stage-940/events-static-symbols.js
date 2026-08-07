const assert = require("assert");
const events = require("events");

assert.strictEqual(typeof events.errorMonitor, "symbol");
assert.strictEqual(typeof events.captureRejections, "boolean");
assert.strictEqual(
  events.EventEmitter.captureRejections,
  events.captureRejections,
);
