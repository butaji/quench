"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(
  typeof processApi.setUncaughtExceptionCaptureCallback,
  "function",
);
assert.strictEqual(
  typeof processApi.hasUncaughtExceptionCaptureCallback,
  "function",
);
assert.strictEqual(processApi.hasUncaughtExceptionCaptureCallback(), false);
processApi.setUncaughtExceptionCaptureCallback(() => {});
assert.strictEqual(processApi.hasUncaughtExceptionCaptureCallback(), true);
processApi.setUncaughtExceptionCaptureCallback(null);
assert.strictEqual(processApi.hasUncaughtExceptionCaptureCallback(), false);

console.log("process exception capture passed");
