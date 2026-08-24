"use strict";
const assert = require("assert");
const { Buffer } = require("buffer");

function allocate(size) {
  return Buffer.alloc(size);
}
function forward(size) {
  return allocate(size);
}

const value = forward(16);
assert.strictEqual(value.length, 16);
assert.strictEqual(value[0], 0);
