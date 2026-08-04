"use strict";

const assert = require("assert");
const wasi = require("node:wasi");

assert.strictEqual(typeof wasi.WASI, "function");
assert.strictEqual(typeof wasi.getImportObject, "function");
assert.strictEqual(typeof wasi.WASI_VERSION, "string");
assert.strictEqual(typeof wasi.WASI_PREVIEW1, "string");

console.log("wasi api passed");
