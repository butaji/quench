const assert = require("node:assert");

assert.deepEqual({ value: 1 }, { value: 1 });
assert.throws(() => assert.deepEqual({ value: 1 }, { value: 2 }));

console.log("assert.deepEqual passed");
