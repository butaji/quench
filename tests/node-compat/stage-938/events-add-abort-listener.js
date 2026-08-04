const assert = require("assert");
const events = require("events");

const controller = new AbortController();
let calls = 0;
const disposable = events.addAbortListener(controller.signal, () => {
  calls += 1;
});

assert.strictEqual(typeof disposable[Symbol.dispose], "function");
controller.abort();
assert.strictEqual(calls, 1);
disposable[Symbol.dispose]();
controller.abort();
assert.strictEqual(calls, 1);
