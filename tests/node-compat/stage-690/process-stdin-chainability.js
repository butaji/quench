"use strict";

const assert = require("assert");
const processApi = require("process");

const listener = () => {};
assert.strictEqual(processApi.stdin.on("data", listener), processApi.stdin);
processApi.stdin.removeListener("data", listener);

console.log("process stdin chainability passed");
