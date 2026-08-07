const assert = require("node:assert");
const url = require("node:url");

for (const value of [null, undefined, 1, {}, true]) {
  assert.throws(() => url.fileURLToPath(value), `invalid value: ${value}`);
}
assert.throws(() => url.fileURLToPath("https://example.com/file"));
assert.throws(() => url.fileURLToPath("file://host/file"));
assert.strictEqual(url.fileURLToPath("file:///"), "/");
console.log("file path error matrix passed");
