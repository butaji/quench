"use strict";
const assert = require("assert");
const { test } = require("node:test");

test("mock fn context", (context) => {
  const fn = context.mock.fn();
  fn(1);
  fn(2);
  assert.strictEqual(fn.mock.callCount(), 2);
});
console.log("node test mock fn passed");
