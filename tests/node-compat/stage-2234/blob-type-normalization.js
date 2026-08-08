const assert = require("assert");

assert.strictEqual(new Blob([], { type: 1 }).type, "1");
assert.strictEqual(new Blob([], { type: false }).type, "false");
assert.strictEqual(new Blob([], { type: {} }).type, "[object object]");
assert.strictEqual(
  new (require("buffer").Blob)([], { type: {} }).type,
  "[object object]"
);
console.log("blob type normalization passed");
