"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.setSourceMapsEnabled, "function");
assert.strictEqual(typeof processApi.sourceMapsEnabled, "boolean");
assert.strictEqual(processApi.setSourceMapsEnabled(true), undefined);
assert.strictEqual(typeof processApi.sourceMapsEnabled, "boolean");

console.log("process source maps passed");
