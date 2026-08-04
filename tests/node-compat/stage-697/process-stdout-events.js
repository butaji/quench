"use strict";

const assert = require("assert");
const processApi = require("process");

const listener = () => {};
assert.strictEqual(processApi.stdout.on("drain", listener), processApi.stdout);
processApi.stdout.removeListener("drain", listener);

console.log("process stdout events passed");
