const assert = require("node:assert");
const asyncHooks = require("node:async_hooks");

const events = [];
const hook = asyncHooks
  .createHook({
    init(asyncId, type) {
      if (type === "PROMISE") events.push(["init", asyncId]);
    },
    promiseResolve(asyncId) {
      events.push(["resolve", asyncId]);
    },
  })
  .enable();

const promise = Promise.resolve(42);
assert.strictEqual(Promise.resolve(promise), promise);
const id = events[0]?.[1];
assert.deepStrictEqual(events, [
  ["init", id],
  ["resolve", id],
]);

hook.disable();
console.log("promise resolve identity passed");
