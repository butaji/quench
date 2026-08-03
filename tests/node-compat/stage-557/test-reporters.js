"use strict";

const assert = require("assert");

assert.throws(() => require("node:test/reporters"), {
  code: "ERR_UNKNOWN_BUILTIN_MODULE"
});

console.log("test reporters error passed");
