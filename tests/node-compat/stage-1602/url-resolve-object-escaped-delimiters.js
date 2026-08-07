const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://example/x%3Fy/z", "g", "http://example/x%3Fy/g"],
  ["http://example/x%23y/z", "g", "http://example/x%23y/g"],
  ["http://example/x/y", "q%3Ar", "http://example/x/q%3Ar"],
  ["http://example/x/y%2Fz", "g/../h", "http://example/x/h"],
  ["http://example/x/y", "%2E%2E/g", "http://example/x/%2E%2E/g"],
  ["http://example/x/y", "./%2E/g", "http://example/x/%2E/g"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("escaped delimiter matrix passed");
