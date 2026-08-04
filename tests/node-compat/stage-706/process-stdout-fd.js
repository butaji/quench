"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.fd, "number");
assert(Number.isInteger(processApi.stdout.fd));
assert(processApi.stdout.fd >= 0);

console.log("process stdout fd passed");
