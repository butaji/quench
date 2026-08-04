"use strict";

const assert = require("assert");
const processApi = require("process");

const listener = () => {};
assert.strictEqual(processApi.on("stage-673-on", listener), processApi);
assert.strictEqual(processApi.once("stage-673-once", listener), processApi);
processApi.removeListener("stage-673-on", listener);
processApi.removeListener("stage-673-once", listener);

console.log("process event chainability passed");
