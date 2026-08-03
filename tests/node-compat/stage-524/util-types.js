"use strict";

const assert = require("assert");
const types = require("util/types");

assert.strictEqual(types.isArrayBuffer(new ArrayBuffer(1)), true);
assert.strictEqual(types.isArrayBufferView(new Uint8Array(1)), true);
assert.strictEqual(types.isDataView(new DataView(new ArrayBuffer(1))), true);
assert.strictEqual(types.isTypedArray(new Uint16Array(1)), true);
assert.strictEqual(types.isUint8Array(new Uint8Array(1)), true);
assert.strictEqual(types.isMap(new Map()), true);
assert.strictEqual(types.isSet(new Set()), true);
assert.strictEqual(types.isPromise(Promise.resolve()), true);
assert.strictEqual(types.isRegExp(/x/), true);
assert.strictEqual(types.isDate(new Date()), true);
assert.strictEqual(types.isArrayBuffer(null), false);

console.log("util types passed");
