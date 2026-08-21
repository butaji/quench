const assert = require("assert");

assert.strictEqual(URL.canParse("https://example.com"), true);
assert.strictEqual(URL.canParse("/path", "https://example.com"), true);
assert.strictEqual(URL.canParse("not a URL"), false);
assert.strictEqual(URL.canParse(":::invalid"), false);

const parsed = URL.parse("/path?q=1", "https://example.com");
assert(parsed instanceof URL);
assert.strictEqual(parsed.href, "https://example.com/path?q=1");
assert.strictEqual(URL.parse(":::invalid"), null);
