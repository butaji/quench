const assert = require("assert");
const path = require("path");

assert.strictEqual(path.matchesGlob("/foo/bar", "/foo/*"), true);
assert.strictEqual(path.matchesGlob("/foo/bar.js", "/foo/*.js"), true);
assert.strictEqual(path.matchesGlob("/foo/b", "/?oo/b"), true);
assert.strictEqual(path.matchesGlob("/foo/bar", "/baz/*"), false);
assert.strictEqual(path.matchesGlob("/foo/bar", "/foo/**/bar"), true);
assert.strictEqual(path.matchesGlob("/foo/a/b/bar", "/foo/**/bar"), true);
assert.strictEqual(path.matchesGlob("/foo/bar", "/foo/{a,b}/bar"), false);
assert.strictEqual(path.matchesGlob("/foo/a/bar", "/foo/{a,b}/bar"), true);
assert.strictEqual(path.matchesGlob("/foo/bar", "**"), true);
assert.strictEqual(
  path.matchesGlob("C:\\path\\to\\file", "C:/path/*/file"),
  true,
);

console.log("path matchesGlob passed");
