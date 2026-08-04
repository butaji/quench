"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.noDeprecation, "boolean");
assert.strictEqual(typeof processApi.traceDeprecation, "boolean");
assert.strictEqual(typeof processApi.throwDeprecation, "boolean");
assert.strictEqual(processApi.noDeprecation, false);
assert.strictEqual(processApi.traceDeprecation, false);
assert.strictEqual(processApi.throwDeprecation, false);

console.log("process deprecation flags passed");
