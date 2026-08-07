"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr._isStdio, true);
assert.strictEqual(typeof processApi.stderr.destroySoon, "function");
assert.strictEqual(processApi.stderr.ref(), processApi.stderr);
assert.strictEqual(processApi.stderr.unref(), processApi.stderr);
assert.strictEqual(
  processApi.stderr.setDefaultEncoding("utf8"),
  processApi.stderr,
);

console.log("process stderr stdio methods passed");
