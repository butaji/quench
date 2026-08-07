const assert = require("assert");
const controller = new AbortController();
assert.doesNotThrow(() => controller.signal.throwIfAborted());
let called = false;
controller.signal.onabort = () => {
  called = true;
};
controller.abort("reason");
assert.throws(
  () => controller.signal.throwIfAborted(),
  (error) => error === "reason",
);
assert.strictEqual(called, true);
assert.strictEqual(controller.signal.dispatchEvent({ type: "abort" }), true);
