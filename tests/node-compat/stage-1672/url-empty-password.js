const assert = require("node:assert");

const url = new URL("https://test:@test");
assert.strictEqual(url.href, "https://test@test/");
assert.strictEqual(url.username, "test");
assert.strictEqual(url.password, "");
console.log("URL empty password serialization passed");
