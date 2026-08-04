"use strict";

const assert = require("assert");
const stream = require("node:stream");

for (const name of ["Readable", "Writable", "Duplex"]) {
  assert.strictEqual(typeof stream[name], "function");
  assert.strictEqual(typeof stream[name].toWeb, "function");
  assert.strictEqual(typeof stream[name].fromWeb, "function");
}

console.log("stream adapters api passed");
