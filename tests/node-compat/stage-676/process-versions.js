"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.versions && typeof processApi.versions === "object");
assert.strictEqual(typeof processApi.versions.node, "string");
assert(processApi.versions.node.length > 0);

console.log("process versions passed");
