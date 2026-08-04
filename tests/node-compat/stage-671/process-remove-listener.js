"use strict";

const assert = require("assert");
const processApi = require("process");

let removed = 0;
let retained = 0;
const removedListener = () => {
  removed += 1;
};
const retainedListener = () => {
  retained += 1;
};
processApi.on("stage-671", removedListener);
processApi.on("stage-671", retainedListener);
processApi.removeListener("stage-671", removedListener);
processApi.emit("stage-671");
assert.strictEqual(removed, 0);
assert.strictEqual(retained, 1);
processApi.removeListener("stage-671", retainedListener);

console.log("process removeListener passed");
