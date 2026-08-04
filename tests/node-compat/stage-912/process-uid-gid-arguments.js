"use strict";

const assert = require("assert");

assert.strictEqual(typeof process.getuid, "function");
assert.strictEqual(typeof process.getgid, "function");
assert.throws(() => process.setuid({}), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => process.setgid({}), { code: "ERR_INVALID_ARG_TYPE" });
assert.strictEqual(process.setuid(0), undefined);
assert.strictEqual(process.setgid(0), undefined);

console.log("process UID GID arguments passed");
