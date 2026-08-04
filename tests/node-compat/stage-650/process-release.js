"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.release && typeof processApi.release === "object");
for (const key of ["name", "sourceUrl", "headersUrl"]) {
  assert.strictEqual(typeof processApi.release[key], "string");
}

console.log("process release passed");
