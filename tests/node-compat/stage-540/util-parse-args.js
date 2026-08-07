"use strict";

const assert = require("assert");
const { parseArgs } = require("util");

const parsed = parseArgs({
  args: ["--name", "quench", "--verbose", "--no-color", "file.js"],
  options: {
    name: { type: "string" },
    verbose: { type: "boolean" },
    color: { type: "boolean" },
  },
  tokens: true,
});
assert.deepStrictEqual(parsed.values, {
  name: "quench",
  verbose: true,
  color: false,
});
assert.deepStrictEqual(parsed.positionals, ["file.js"]);
assert.strictEqual(parsed.tokens.length, 4);

console.log("util parse args passed");
