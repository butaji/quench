"use strict";

const assert = require("assert");
const consumers = require("node:stream/consumers");

for (const name of ["arrayBuffer", "blob", "buffer", "json", "text"]) {
  assert.strictEqual(typeof consumers[name], "function");
}

console.log("stream consumers api passed");
