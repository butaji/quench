"use strict";

const assert = require("assert");
const processApi = require("process");

for (const stream of [processApi.stdout, processApi.stderr]) {
  assert.strictEqual(typeof stream[Symbol.asyncDispose], "function");
  assert.strictEqual(
    stream[Symbol.asyncDispose].constructor.name,
    "AsyncFunction",
  );
}

console.log("process stdio async dispose passed");
