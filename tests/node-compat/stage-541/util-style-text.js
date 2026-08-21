"use strict";

const assert = require("assert");
const { styleText } = require("util");

assert.strictEqual(styleText("green", "ok"), "\u001b[32mok\u001b[39m");
assert.strictEqual(
  styleText(["bold", "yellow"], "warn"),
  "\u001b[1m\u001b[33mwarn\u001b[39m\u001b[22m",
);
assert.strictEqual(styleText("red", "plain", { colors: false }), "plain");

console.log("util style text passed");
