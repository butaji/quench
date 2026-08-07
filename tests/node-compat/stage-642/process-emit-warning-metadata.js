"use strict";

const assert = require("assert");
const processApi = require("process");

let warning;
const listener = (value) => (warning = value);
processApi.once("warning", listener);
processApi.emitWarning("stage warning", {
  name: "StageWarning",
  code: "STAGE_642",
});

assert(warning && typeof warning === "object");
assert.strictEqual(warning.name, "StageWarning");
assert.strictEqual(warning.message, "stage warning");
assert.strictEqual(warning.code, "STAGE_642");

console.log("process emitWarning metadata passed");
