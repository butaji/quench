"use strict";

const assert = require("assert");
const url = require("node:url");

assert.strictEqual(typeof url.URLPattern, "function");
const pattern = new url.URLPattern({ pathname: "/users/:id" });
assert.strictEqual(pattern.test("https://example.test/users/42"), true);
assert.strictEqual(
  pattern.exec("https://example.test/users/42").pathname.groups.id,
  "42",
);

console.log("url pattern api passed");
