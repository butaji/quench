const assert = require("node:assert");
const url = require("node:url");
const cases = [
  ["/foo/bar/baz", "/../etc/passwd", "/etc/passwd"],
  ["http://localhost", "file:///Users/foo", "file:///Users/foo"],
  ["http://localhost", "file://foo/Users", "file://foo/Users"],
];
for (const [from, to, expected] of cases) {
  assert.deepStrictEqual(
    url.resolveObject(url.parse(from), to),
    url.parse(expected),
    `${from} + ${to}`,
  );
}
console.log("parsed file targets passed");
