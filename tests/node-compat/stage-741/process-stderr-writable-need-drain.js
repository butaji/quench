"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.writableNeedDrain, false);

console.log("process stderr writableNeedDrain passed");
