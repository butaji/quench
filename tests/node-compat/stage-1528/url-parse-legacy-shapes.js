const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.parse("http:/baz/../foo/bar").slashes, null);
assert.strictEqual(url.parse("file:///etc/passwd").href, "file:///etc/passwd");
assert.strictEqual(url.parse("file://localhost/etc/passwd").host, "localhost");
assert.strictEqual(
  url.parse("<http://goo.corn/bread> Is a URL!").pathname,
  "%3Chttp://goo.corn/bread%3E%20Is%20a%20URL!",
);
console.log("legacy URL object shapes passed");
