"use strict";

const assert = require("assert");

assert.throws(() => require("wasi"), {
  code: "ERR_UNKNOWN_BUILTIN_MODULE"
});
assert.throws(() => require("node:wasi"), {
  code: "ERR_UNKNOWN_BUILTIN_MODULE"
});

console.log("wasi error passed");
