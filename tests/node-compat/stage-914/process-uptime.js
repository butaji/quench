"use strict";

const assert = require("assert");

const first = process.uptime();
assert.ok(Number.isFinite(first));
assert.ok(first >= 0);
const second = process.uptime();
assert.ok(second >= first);

console.log("process uptime passed");
