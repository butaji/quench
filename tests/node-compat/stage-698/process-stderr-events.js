"use strict";

const assert = require("assert");
const processApi = require("process");

const listener = () => {};
assert.strictEqual(processApi.stderr.on("drain", listener), processApi.stderr);
processApi.stderr.removeListener("drain", listener);

console.log("process stderr events passed");
