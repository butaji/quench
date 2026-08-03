const assert = require("assert");
const url = new URL("https://example.test:8443/a");
assert.strictEqual(url.hostname, "example.test");
assert.strictEqual(url.port, "8443");
assert.strictEqual(url.host, "example.test:8443");
assert.strictEqual(url.origin, "https://example.test:8443");
assert.strictEqual(url.href, "https://example.test:8443/a");
