const assert = require("node:assert");

const key = Symbol("key");
assert.strictEqual(process.env[key], undefined);
assert.throws(() => {
  process.env[key] = 42;
}, TypeError);
assert.throws(() => {
  process.env.VALUE = key;
}, TypeError);
assert.strictEqual(key in process.env, false);
assert.strictEqual(delete process.env[key], true);
Object.prototype.toString.call(process.env);
