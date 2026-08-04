"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.traceDeprecation, "boolean");
const previous = processApi.traceDeprecation;
processApi.traceDeprecation = !previous;
assert.strictEqual(processApi.traceDeprecation, !previous);
processApi.traceDeprecation = previous;

console.log("process traceDeprecation passed");
