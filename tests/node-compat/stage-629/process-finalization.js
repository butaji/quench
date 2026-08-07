"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.finalization && typeof processApi.finalization === "object");
assert.strictEqual(typeof processApi.finalization.register, "function");
assert.strictEqual(typeof processApi.finalization.unregister, "function");
assert.strictEqual(
  typeof processApi.finalization.registerBeforeExit,
  "function",
);
assert.strictEqual(
  processApi.finalization.register({}, () => {}),
  undefined,
);

console.log("process finalization passed");
