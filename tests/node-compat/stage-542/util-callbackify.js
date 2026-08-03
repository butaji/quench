"use strict";

const assert = require("assert");
const { callbackify } = require("util");

let received;
callbackify((value) => value * 2)(3, (error, value) => {
  received = { error, value };
});
callbackify(async () => {
  throw new Error("failed");
})((error) => {
  assert.strictEqual(error.message, "failed");
});

setTimeout(() => {
  assert.ifError(received.error);
  assert.strictEqual(received.value, 6);
  assert.throws(() => callbackify(1));
  console.log("util callbackify passed");
}, 10);
