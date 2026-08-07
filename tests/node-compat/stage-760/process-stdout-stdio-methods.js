"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout._isStdio, true);
assert.strictEqual(typeof processApi.stdout.destroySoon, "function");
assert.strictEqual(processApi.stdout.ref(), processApi.stdout);
assert.strictEqual(processApi.stdout.unref(), processApi.stdout);
assert.strictEqual(
  processApi.stdout.setDefaultEncoding("utf8"),
  processApi.stdout,
);

console.log("process stdout stdio methods passed");
