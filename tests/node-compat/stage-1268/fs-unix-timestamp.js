const assert = require("node:assert");
const fs = require("node:fs");

assert.strictEqual(fs._toUnixTimestamp(12), 12);
assert.strictEqual(fs._toUnixTimestamp(new Date(12000)), 12);
assert.ok(fs._toUnixTimestamp(-1) > 0);

console.log("fs unix timestamp passed");
