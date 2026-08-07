const assert = require("assert");
const fs = require("fs");
assert.strictEqual(typeof fs.glob, "function");
assert.strictEqual(typeof fs.globSync, "function");
assert.strictEqual(typeof fs.promises.glob, "function");
console.log(fs.globSync("*.definitely-not-present"));
