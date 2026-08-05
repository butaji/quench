const assert = require("node:assert");
const fs = require("node:fs");

assert.notStrictEqual(fs.constants.S_IRUSR, undefined);
assert.notStrictEqual(fs.constants.S_IWUSR, undefined);
assert.strictEqual(Object.getPrototypeOf(fs.constants), null);
assert.strictEqual(fs.constants.O_RDONLY, 0);
assert.strictEqual(fs.constants.S_IFDIR, 0o40000);

console.log("fs constants passed");
