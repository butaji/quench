"use strict";

const assert = require("assert");

const original = process.umask();
const previous = process.umask("0664");
assert.strictEqual(typeof previous, "number");
assert.strictEqual(process.umask(previous), 0o664);
assert.strictEqual(process.umask(), previous);
assert.throws(() => process.umask({}), { code: "ERR_INVALID_ARG_TYPE" });
assert.throws(() => process.umask("999"), { code: "ERR_INVALID_ARG_VALUE" });
process.umask(original);

console.log("process umask passed");
