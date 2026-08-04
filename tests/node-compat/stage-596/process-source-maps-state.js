"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.sourceMapsEnabled, "boolean");
assert.strictEqual(processApi.sourceMapsEnabled, false);

console.log("process source maps state passed");
