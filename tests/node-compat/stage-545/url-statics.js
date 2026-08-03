"use strict";

const assert = require("assert");

assert.strictEqual(URL.canParse("https://example.com/path"), true);
assert.strictEqual(URL.canParse("/path", "https://example.com"), true);
assert.strictEqual(URL.canParse("not a url"), false);
assert.strictEqual(URL.parse("https://example.com/path").pathname, "/path");
assert.strictEqual(URL.parse("not a url"), null);

console.log("url statics passed");
