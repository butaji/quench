const assert = require("node:assert");
const fs = require("node:fs");

const link = "chmod-symlink-target";
const target = "chmod-symlink-file";
fs.writeFileSync(target, "data");
fs.symlinkSync(target, link);
fs.lchmodSync(link, 0o600);
assert.strictEqual(fs.lstatSync(link).mode & 0o777, 0o600);
console.log("chmod symlink passed");
