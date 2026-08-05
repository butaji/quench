const assert = require("node:assert");
const fs = require("node:fs");

const first = fs.openSync(__filename, "r");
const second = fs.openSync(__filename, "r");
assert.notStrictEqual(first, second);
fs.closeSync(first);
fs.closeSync(second);

console.log("filesystem descriptor allocation passed");
