"use strict";

const assert = require("assert");

for (const name of ["node:sqlite", "sqlite"]) {
  assert.throws(() => require(name), { code: "ERR_UNKNOWN_BUILTIN_MODULE" });
}

console.log("sqlite error passed");
