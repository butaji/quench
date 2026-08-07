"use strict";

const assert = require("assert");
const processApi = require("process");

for (
  const name of [
    "getgroups",
    "initgroups",
    "setgroups",
    "setegid",
    "seteuid",
    "getegid",
    "geteuid",
  ]
) {
  assert.strictEqual(typeof processApi[name], "function");
}
assert.strictEqual(Array.isArray(processApi.getgroups()), true);
assert.strictEqual(typeof processApi.getegid(), "number");
assert.strictEqual(typeof processApi.geteuid(), "number");

console.log("process credential methods passed");
