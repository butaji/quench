const assert = require("assert");
const { EventEmitter, on } = require("events");

(async () => {
  const emitter = new EventEmitter();
  process.nextTick(() => emitter.emit("value", 42));
  for await (const args of on(emitter, "value")) {
    assert.deepStrictEqual(args, [42]);
    break;
  }
  assert.strictEqual(emitter.listenerCount("value"), 0);
})();
