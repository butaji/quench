"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.write(""), true);

console.log("process stderr write passed");
