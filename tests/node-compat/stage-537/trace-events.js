"use strict";

const assert = require("assert");

assert.throws(() => require("trace_events"), {
  code: "ERR_UNKNOWN_BUILTIN_MODULE"
});
assert.throws(() => require("node:trace_events"), {
  code: "ERR_UNKNOWN_BUILTIN_MODULE"
});

console.log("trace events error passed");
