"use strict";

const assert = require("assert");
const processApi = require("process");

const listener = () => {};
assert.strictEqual(processApi.on("stage-645", listener), processApi);
assert.strictEqual(processApi.once("stage-645-once", listener), processApi);
processApi.removeListener("stage-645", listener);
processApi.removeAllListeners("stage-645-once");

console.log("process event chainability passed");
