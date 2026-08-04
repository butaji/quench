"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.release, "object");
assert.strictEqual(processApi.release.name, "node");
assert.strictEqual(typeof processApi.release.sourceUrl, "string");
assert.strictEqual(typeof processApi.release.headersUrl, "string");

console.log("process release passed");
