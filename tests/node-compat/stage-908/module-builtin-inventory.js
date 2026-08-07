"use strict";

const assert = require("assert");
const { builtinModules } = require("module");

assert.ok(builtinModules.includes("http"));
assert.ok(builtinModules.includes("sys"));
assert.deepStrictEqual(
  builtinModules.filter((name) => name.startsWith("internal/")),
  [],
);

console.log("module builtin inventory passed");
