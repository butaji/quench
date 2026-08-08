const assert = require("assert");

const originalUid = process.getuid();
const originalGid = process.getgid();
process.setuid("nobody");
process.setgid("65534");
assert.strictEqual(process.getuid(), 65534);
assert.strictEqual(process.getgid(), 65534);
process.setuid(originalUid);
process.setgid(originalGid);
assert.strictEqual(process.getuid(), originalUid);
assert.strictEqual(process.getgid(), originalGid);
console.log("process credentials passed");
