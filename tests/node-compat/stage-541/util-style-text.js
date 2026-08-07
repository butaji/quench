"use strict";

const assert = require("assert");
const { styleText } = require("util");

assert.strictEqual(styleText("ok", "green"), "\u001b[32mok\u001b[39m");
assert.strictEqual(
  styleText("warn", ["bold", "yellow"]),
  "\u001b[33m\u001b[1mwarn\u001b[22m\u001b[39m",
);
assert.strictEqual(styleText("plain", "red", { colors: false }), "plain");

console.log("util style text passed");
