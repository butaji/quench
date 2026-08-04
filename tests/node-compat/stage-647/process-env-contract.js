"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.env && typeof processApi.env === "object");
const key = "__QUENCH_STAGE_647";
processApi.env[key] = 647;
assert.strictEqual(processApi.env[key], "647");
delete processApi.env[key];
assert.strictEqual(processApi.env[key], undefined);

console.log("process env contract passed");
