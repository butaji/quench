const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["http://example/x/y%2Fz", "abc", "http://example/x/abc"],
  ["http://example/a/x/y/z", "../../x%2Fabc", "http://example/a/x%2Fabc"],
  ["http://example/a/x/y%2Fz", "../x%2Fabc", "http://example/a/x%2Fabc"],
  ["http://example/x%2Fy/z", "abc", "http://example/x%2Fy/abc"],
  ["http://ex/x/y", "q%3Ar", "http://ex/x/q%3Ar"],
  ["http://example/x/y%2Fz", "/x%2Fabc", "http://example/x%2Fabc"],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("encoded path matrix passed");
