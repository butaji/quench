"use strict";

const assert = require("assert");
const os = require("node:os");

assert.strictEqual(typeof os.EOL, "string");
assert.strictEqual(typeof os.devNull, "string");
for (const name of ["homedir", "tmpdir", "platform", "arch"]) {
  assert.strictEqual(typeof os[name], "function");
}
assert.strictEqual(typeof os.homedir(), "string");
assert.strictEqual(typeof os.tmpdir(), "string");

console.log("os platform api passed");
