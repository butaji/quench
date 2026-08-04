"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.title, "string");
const previous = processApi.title;
processApi.title = "quench-node-stage-658";
assert.strictEqual(processApi.title, "quench-node-stage-658");
processApi.title = previous;

console.log("process title passed");
