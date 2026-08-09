const assert = require("assert");
const url = require("url");

for (const input of ["//some_path", "http://example.com/", "mailto:a@b"]) {
  const parsed = url.parse(input);
  assert.ok(parsed instanceof url.Url);
  assert.strictEqual(Object.getPrototypeOf(parsed), url.Url.prototype);
}

const resolved = url
  .parse("http://nodejs.org/")
  .resolveObject("javascript:alert(1)");
assert.ok(resolved instanceof url.Url);

assert.deepStrictEqual(
  url.parse(" \t http://user:pw@example.com/\n"),
  url.parse("http://user:pw@example.com/")
);

const pathOnly = url.parse('foo bar?x="y"#z z');
assert.strictEqual(pathOnly.pathname, "foo%20bar");
assert.strictEqual(pathOnly.search, "?x=%22y%22");
assert.strictEqual(pathOnly.hash, "#z%20z");
assert.strictEqual(pathOnly.href, "foo%20bar?x=%22y%22#z%20z");

assert.strictEqual(
  url.parse("http://example.com/a=b&c+d,e;f$g").pathname,
  "/a=b&c+d,e;f$g"
);
assert.strictEqual(
  url.parse("http:/baz/../foo/bar").href,
  "http:/baz/../foo/bar"
);
assert.strictEqual(
  url.format({ protocol: "http:", pathname: "/baz/../foo/bar" }),
  "http:/baz/../foo/bar"
);
assert.strictEqual(
  url.format({ protocol: "file:", pathname: "/home/user" }),
  "file:///home/user"
);

const relativeAuthority = url.parse("//user:pass@example.com:8000/path?q#h");
assert.strictEqual(relativeAuthority.protocol, null);
assert.strictEqual(relativeAuthority.slashes, true);
assert.strictEqual(relativeAuthority.auth, "user:pass");
assert.strictEqual(relativeAuthority.host, "example.com:8000");
assert.strictEqual(
  relativeAuthority.href,
  "//user:pass@example.com:8000/path?q#h"
);
