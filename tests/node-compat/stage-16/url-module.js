const assert = require("assert");
const { fileURLToPath, pathToFileURL, URL } = require("node:url");
const file = pathToFileURL("/tmp/quench node.txt");
assert.strictEqual(fileURLToPath(file), "/tmp/quench node.txt");
assert.strictEqual(file instanceof URL, true);
assert.strictEqual(
  new URL("/child", "https://example.test/base").href,
  "https://example.test/child",
);
