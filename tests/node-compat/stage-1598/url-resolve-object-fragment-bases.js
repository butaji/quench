const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["#hash2", "#hash1", "#hash1"],
  ["#hash2", "", "#hash2"],
  ["#hash2", "foo", "foo"],
  ["http://example/x/y", "#hash1", "http://example/x/y#hash1"],
  ["http://example/x/y#old", "#hash1", "http://example/x/y#hash1"],
  ["http://example/x/y#old", "", "http://example/x/y#old"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("fragment base matrix passed");
