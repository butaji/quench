"use strict";

const assert = require("assert");
let stage = -1;
let threw = false;
process.once("uncaughtException", (error, origin) => {
  assert.strictEqual(origin, "uncaughtException");
  assert.strictEqual(stage, 0);
  assert.strictEqual(error.message, "setImmediate Err");
});
const run = (value) => {
  assert(value >= stage);
  stage = value;
  if (!threw) setImmediate(run, 2);
};
for (let i = 0; i < 10; i++) setImmediate(run, 0);
setImmediate(() => {
  threw = true;
  process.nextTick(() => assert.strictEqual(stage, 1));
  throw new Error("setImmediate Err");
});
for (let i = 0; i < 10; i++) setImmediate(run, 1);
