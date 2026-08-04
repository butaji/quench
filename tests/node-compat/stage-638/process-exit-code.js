"use strict";

const assert = require("assert");
const processApi = require("process");

processApi.exitCode = 17;
assert.strictEqual(processApi.exitCode, 17);
processApi.exitCode = undefined;
assert.strictEqual(processApi.exitCode, undefined);

console.log("process exitCode passed");
