"use strict";

const assert = require("assert");
const processApi = require("process");

for (const stream of [processApi.stdout, processApi.stderr]) {
  assert.strictEqual(stream[Symbol.asyncDispose]() instanceof Promise, true);
}

console.log("process stdio async dispose result passed");
