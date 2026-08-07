"use strict";

const assert = require("assert");
const utilApi = require("node:util");

for (
  const name of [
    "format",
    "inspect",
    "promisify",
    "callbackify",
    "inherits",
    "parseArgs",
    "parseEnv",
    "MIMEType",
    "TextDecoder",
    "isDeepStrictEqual",
  ]
) {
  assert.strictEqual(typeof utilApi[name], "function");
}
assert.strictEqual(typeof utilApi.types, "object");
assert.strictEqual(utilApi.format("value: %s", "ok"), "value: ok");

console.log("util core api passed");
