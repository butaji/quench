"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.noDeprecation, "boolean");
const previous = processApi.noDeprecation;
processApi.noDeprecation = !previous;
assert.strictEqual(processApi.noDeprecation, !previous);
processApi.noDeprecation = previous;

console.log("process noDeprecation passed");
