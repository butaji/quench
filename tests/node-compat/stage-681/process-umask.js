"use strict";

const assert = require("assert");
const processApi = require("process");

const current = processApi.umask();
assert.strictEqual(typeof current, "number");
assert(Number.isInteger(current));
assert(current >= 0);
assert.strictEqual(processApi.umask(current), current);

console.log("process umask passed");
