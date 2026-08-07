const assert = require("assert");
const { parse } = require("url");

assert.throws(() => parse(null), {
  code: "ERR_INVALID_ARG_TYPE",
  name: "TypeError",
});

const parsed = parse("https://example.com:8443/path?query=value#hash");
assert.strictEqual(parsed.protocol, "https:");
assert.strictEqual(parsed.host, "example.com:8443");
assert.strictEqual(parsed.hostname, "example.com");
assert.strictEqual(parsed.port, "8443");
assert.strictEqual(parsed.pathname, "/path");
assert.strictEqual(parsed.search, "?query=value");
assert.strictEqual(parsed.query, "query=value");
assert.strictEqual(parsed.hash, "#hash");
assert.strictEqual(parsed.path, "/path?query=value");
