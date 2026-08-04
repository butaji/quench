"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.throwDeprecation, "boolean");
const previous = processApi.throwDeprecation;
processApi.throwDeprecation = !previous;
assert.strictEqual(processApi.throwDeprecation, !previous);
processApi.throwDeprecation = previous;

console.log("process throwDeprecation passed");
