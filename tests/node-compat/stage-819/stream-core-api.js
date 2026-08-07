"use strict";

const assert = require("assert");
const streamApi = require("node:stream");

for (
  const name of [
    "Stream",
    "Readable",
    "Writable",
    "Duplex",
    "Transform",
    "PassThrough",
    "pipeline",
    "finished",
    "addAbortSignal",
    "compose",
    "setDefaultHighWaterMark",
    "getDefaultHighWaterMark",
  ]
) {
  assert.strictEqual(typeof streamApi[name], "function");
}

console.log("stream core api passed");
