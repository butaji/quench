const assert = require("assert");

const first = require("url");
const firstParse = first.parse;
const second = require("node:url");

assert.strictEqual(second.parse, firstParse);
for (const value of ["//some_path", "https://example.com/path", "mailto:a@b"]) {
  const parsed = second.parse(value);
  assert.strictEqual(typeof parsed.resolve, "function");
  assert.strictEqual(typeof parsed.resolveObject, "function");
}
