"use strict";

const assert = require("assert");
const { promisify } = require("util");

let warnings = 0;
const listener = (warning) => {
  assert.strictEqual(warning.name, "DeprecationWarning");
  assert.strictEqual(warning.code, "DEP0174");
  warnings++;
};
process.on("warning", listener);

function callbackOnly() {}
callbackOnly.constructor = (async () => {}).constructor;
promisify(callbackOnly);

promisify(async (callback) => callback())().then(() => {
  assert.strictEqual(warnings, 1);
  promisify(async () => {})().then(() => {
    assert.strictEqual(warnings, 2);
    process.off("warning", listener);
  });
});
