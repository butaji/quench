"use strict";

const assert = require("assert");
const processApi = require("process");

let warning;
processApi.once("warning", (value) => (warning = value));
processApi.emitWarning(new Error("error warning"));

assert(warning && typeof warning === "object");
assert.strictEqual(warning.name, "Warning");
assert.strictEqual(warning.message, "error warning");

console.log("process emitWarning Error passed");
