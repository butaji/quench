"use strict";

const assert = require("assert");
const processApi = require("process");

let cleared = 0;
let retained = 0;
processApi.on("stage-640-cleared", () => cleared++);
processApi.on("stage-640-retained", () => retained++);
processApi.removeAllListeners("stage-640-cleared");

assert.strictEqual(processApi.emit("stage-640-cleared"), false);
assert.strictEqual(processApi.emit("stage-640-retained"), true);
assert.strictEqual(cleared, 0);
assert.strictEqual(retained, 1);
processApi.removeAllListeners("stage-640-retained");

console.log("process removeAllListeners passed");
