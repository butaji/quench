"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.fd, "number");
assert(Number.isInteger(processApi.stderr.fd));
assert(processApi.stderr.fd >= 0);

console.log("process stderr fd passed");
