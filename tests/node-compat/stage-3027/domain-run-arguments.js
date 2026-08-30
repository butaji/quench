"use strict";

const assert = require("assert");
const domain = require("domain").create();

assert.strictEqual(domain.run(() => "return value"), "return value");
assert.strictEqual(
  domain.run((first, second) => `${first} ${second}`, "return", "value"),
  "return value",
);
