"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.writableNeedDrain, false);

console.log("process stdout writableNeedDrain passed");
