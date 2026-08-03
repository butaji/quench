const util = require("util");
const assert = require("assert");
assert.strictEqual(typeof util.promisify, "function");
const wrapped = util.promisify((value, callback) => callback(null, value + 1));
wrapped(41).then((value) => {
  assert.strictEqual(value, 42);
});
