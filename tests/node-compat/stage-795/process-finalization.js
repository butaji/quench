"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.finalization, "object");
for (const name of ["register", "registerBeforeExit", "unregister"]) {
  assert.strictEqual(typeof processApi.finalization[name], "function");
}

console.log("process finalization passed");
