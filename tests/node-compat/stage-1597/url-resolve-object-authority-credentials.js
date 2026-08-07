const assert = require("node:assert");
const url = require("node:url");

const cases = [
  [
    "http://asdf:qwer@www.example.com",
    "http://diff:auth@www.example.com",
    "http://diff:auth@www.example.com/",
  ],
  [
    "https://example.com:82/",
    "https://example.com:81/",
    "https://example.com:81/",
  ],
  [
    "https://user:password@example.org/",
    "https://another.host.com/",
    "https://another.host.com/",
  ],
  [
    "https://user:password@example.org/",
    "//another.host.com/",
    "https://another.host.com/",
  ],
  [
    "https://user:password@example.org/",
    "http://another.host.com/",
    "http://another.host.com/",
  ],
  [
    "https://user:password@example.com",
    "https://example.com/foo",
    "https://example.com/foo",
  ],
];
for (const [from, target, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), target),
    url.parse(expected),
    `${from} + ${target}`,
  );
}
console.log("authority credential matrix passed");
