"use strict";

const assert = require("assert");

const key = "QUENCH_STAGE_919";
const previous = process.env[key];
process.env[key] = "present";
assert.strictEqual(process.env[key], "present");
delete process.env[key];
assert.strictEqual(process.env[key], undefined);
if (previous !== undefined) process.env[key] = previous;

console.log("process environment passed");
