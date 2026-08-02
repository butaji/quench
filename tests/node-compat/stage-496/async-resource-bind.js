const assert = require("assert");
const { AsyncResource, executionAsyncId } = require("async_hooks");

const resource = new AsyncResource("test");
const bound = resource.bind(function (value) {
  assert.strictEqual(value, "value");
  return executionAsyncId();
});

assert.strictEqual(bound.length, 1);
assert.strictEqual(bound("value"), resource.asyncId());
const object = {};
const objectBound = resource.bind(function () {
  return this;
}, object);
assert.strictEqual(objectBound(), object);
assert.throws(() => resource.bind(false), { code: "ERR_INVALID_ARG_TYPE" });
