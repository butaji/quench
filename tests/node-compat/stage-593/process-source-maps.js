"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.setSourceMapsEnabled, "function");
assert.strictEqual(processApi.setSourceMapsEnabled(true), undefined);
assert.strictEqual(processApi.setSourceMapsEnabled(false), undefined);

console.log("process source maps passed");
