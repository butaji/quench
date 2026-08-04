"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.versions && typeof processApi.versions === "object");
assert.strictEqual(typeof processApi.versions.node, "string");
assert(/^\d+\.\d+\.\d+$/.test(processApi.versions.node));

console.log("process versions node passed");
