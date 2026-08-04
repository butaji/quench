"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.write(""), true);

console.log("process stdout write passed");
