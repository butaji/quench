"use strict";

const assert = require("assert");
const reporters = require("node:test/reporters");

for (
  const name of [
    "dot",
    "junit",
    "json",
    "lcov",
    "markdown",
    "spec",
    "tap",
    "teamcity",
    "xunit",
  ]
) {
  assert.strictEqual(typeof reporters[name], "function");
}

console.log("test reporters api passed");
