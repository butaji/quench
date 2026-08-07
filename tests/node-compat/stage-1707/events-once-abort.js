const assert = require("assert");
const { once } = require("events");

(async () => {
  const controller = new AbortController();
  const promise = once(controller.signal, "abort");
  controller.abort("reason");
  const [event] = await promise;
  assert.strictEqual(event.type, "abort");
  assert.strictEqual(event.target, controller.signal);
})();
