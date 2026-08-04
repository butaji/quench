"use strict";

const assert = require("assert");
const processApi = require("process");

let selected = 0;
let retained = 0;
processApi.on("stage-670-selected", () => {
  selected += 1;
});
processApi.on("stage-670-retained", () => {
  retained += 1;
});
processApi.removeAllListeners("stage-670-selected");
processApi.emit("stage-670-selected");
processApi.emit("stage-670-retained");
assert.strictEqual(selected, 0);
assert.strictEqual(retained, 1);
processApi.removeAllListeners("stage-670-retained");

console.log("process removeAllListeners passed");
