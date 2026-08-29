const assert = require("assert");
const util = require("util");

assert.strictEqual(
  util.inspect(/a/, { colors: true }),
  "\x1b[32m/\x1b[39m\x1b[33ma\x1b[39m\x1b[32m/\x1b[39m",
);
