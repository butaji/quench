"use strict";

const assert = require("assert");
const types = require("node:util/types");

for (
  const name of [
    "isAnyArrayBuffer",
    "isArgumentsObject",
    "isArrayBuffer",
    "isArrayBufferView",
    "isAsyncFunction",
    "isDate",
    "isMap",
    "isPromise",
    "isRegExp",
    "isSet",
    "isTypedArray",
    "isUint8Array",
  ]
) {
  assert.strictEqual(typeof types[name], "function");
}
assert.strictEqual(types.isDate(new Date()), true);
assert.strictEqual(types.isMap(new Map()), true);

console.log("util types api passed");
