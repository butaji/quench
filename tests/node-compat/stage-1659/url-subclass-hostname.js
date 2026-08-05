const assert = require("node:assert");
const url = new (class extends URL {
  get hostname() {
    return "bar.com";
  }
})("http://foo.com/");
assert.strictEqual(url.href, "http://foo.com/");
assert.strictEqual(url.hostname, "bar.com");
assert.strictEqual(url.toString(), "http://foo.com/");
assert.strictEqual(url.toJSON(), "http://foo.com/");
assert.strictEqual(url.hash, "");
assert.strictEqual(url.host, "foo.com");
assert.strictEqual(url.origin, "http://foo.com");
assert.strictEqual(url.password, "");
assert.strictEqual(url.protocol, "http:");
assert.strictEqual(url.username, "");
assert.strictEqual(url.search, "");
assert.strictEqual(url.searchParams.toString(), "");
console.log("URL subclass hostname passed");
