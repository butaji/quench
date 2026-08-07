const assert = require("node:assert");
const url = require("node:url");

const cases = [
  ["file:/ex/x/y", "ftp://ex/x/q/r", "ftp://ex/x/q/r"],
  ["http://example/x/y", "ftp://ex/x/q/r", "ftp://ex/x/q/r"],
  ["mailto:user@example.org", "http://example/x/y", "http://example/x/y"],
  [
    "http://example/x/y",
    "mailto:another@example.org",
    "mailto:another@example.org",
  ],
  [
    "https://example.com/",
    "http://another.host.com/",
    "http://another.host.com/",
  ],
  [
    "http://example.com/",
    "https://another.host.com/",
    "https://another.host.com/",
  ],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("mixed scheme matrix passed");
