const assert = require("assert");
const vfs = require("node:vfs");
const { R_OK, W_OK, X_OK } = require("fs").constants;

const fs = vfs.create();
fs.writeFileSync("/read-only", "x");
fs.chmodSync("/read-only", 0o444);
assert.throws(() => fs.accessSync("/read-only", W_OK), { code: "EACCES" });
assert.doesNotThrow(() => fs.accessSync("/read-only", R_OK));
fs.writeFileSync("/write-only", "x");
fs.chmodSync("/write-only", 0o222);
assert.throws(() => fs.accessSync("/write-only", R_OK), { code: "EACCES" });
assert.rejects(fs.promises.access("/read-only", X_OK), { code: "EACCES" });
console.log("vfs access permissions passed");
