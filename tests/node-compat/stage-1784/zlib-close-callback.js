"use strict";
const assert = require("assert");
const zlib = require("zlib");

const stream = zlib.createGunzip();
let called = false;
stream.close(() => {
  called = true;
  assert.strictEqual(stream.closed, true);
});
queueMicrotask(() => assert.strictEqual(called, true));
