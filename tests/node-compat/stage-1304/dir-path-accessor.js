const assert = require("node:assert");
const fs = require("node:fs");

const dir = fs.opendirSync(".");
assert.throws(() => fs.Dir.prototype.path, { code: "ERR_INVALID_THIS" });
assert.strictEqual(dir.path, ".");
dir.closeSync();

console.log("dir path accessor passed");
