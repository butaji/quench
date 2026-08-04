"use strict";

const assert = require("assert");
const processApi = require("process");

for (const name of ["umask", "getgid", "getuid", "setgid", "setuid"]) {
  assert.strictEqual(typeof processApi[name], "function");
}
assert.strictEqual(typeof processApi.umask(), "number");
assert.strictEqual(typeof processApi.getgid(), "number");
assert.strictEqual(typeof processApi.getuid(), "number");

console.log("process identity methods passed");
