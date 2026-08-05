const assert = require("node:assert");
const { stringToFlags } = require("internal/fs/utils");
assert.strictEqual(stringToFlags("as"), 1053761);
assert.strictEqual(stringToFlags("sa+"), 1053762);
console.log("open flag sync values passed");
