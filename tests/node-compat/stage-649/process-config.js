"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.config && typeof processApi.config === "object");
assert(
  processApi.config.variables &&
    typeof processApi.config.variables === "object",
);

console.log("process config passed");
