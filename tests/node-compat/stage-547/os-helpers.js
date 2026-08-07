"use strict";

const assert = require("assert");
const os = require("os");

assert.strictEqual(typeof os.homedir(), "string");
assert.strictEqual(typeof os.tmpdir(), "string");
const user = os.userInfo();
for (const key of ["uid", "gid", "username", "homedir", "shell"]) {
  assert.ok(Object.hasOwn(user, key));
}
assert.strictEqual(user.homedir, os.homedir());

console.log("os helpers passed");
