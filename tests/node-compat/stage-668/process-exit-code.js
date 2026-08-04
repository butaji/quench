"use strict";

const assert = require("assert");
const processApi = require("process");

const previous = processApi.exitCode;
processApi.exitCode = 17;
assert.strictEqual(processApi.exitCode, 17);
processApi.exitCode = previous;

console.log("process exit code passed");
