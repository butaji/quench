"use strict";

const assert = require("assert");

assert.strictEqual(typeof URL.canParse, "function");
assert.strictEqual(typeof URL.parse, "function");
assert.strictEqual(URL.canParse("/path", "https://example.com"), true);
assert.strictEqual(URL.canParse(":::invalid"), false);
assert.strictEqual(URL.parse(":::invalid"), null);
assert.strictEqual(URL.parse("/path?q=1", "https://example.com").href, "https://example.com/path?q=1");

console.log("URL static host boundary passed");
