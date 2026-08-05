const assert = require("node:assert");
const fs = require("node:fs");

const path = "fstat-open-descriptor.txt";
const descriptor = fs.openSync(path, "w");
fs.fchmodSync(descriptor, 0o640);
assert.strictEqual(fs.fstatSync(descriptor).mode & 0o777, 0o640);
fs.closeSync(descriptor);
fs.unlinkSync(path);
console.log("fstat descriptor passed");
