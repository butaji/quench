"use strict";

const assert = require("assert");
const processApi = require("process");

for (const [name, value] of Object.entries(processApi.versions)) {
  assert.strictEqual(typeof name, "string");
  assert.strictEqual(typeof value, "string");
  assert(value.length > 0);
}

console.log("process version map passed");
