const assert = require("assert");
const url = require("url");

assert.strictEqual(
  url.format({ protocol: "file:", pathname: "/tmp/example" }),
  "file:///tmp/example",
);

const parsed = url.resolveObject(
  url.parse("file:/tmp/example"),
  "#fragment",
);
assert.strictEqual(parsed.href, "file:/tmp/example#fragment");
assert.strictEqual(url.format(parsed), parsed.href);
