const assert = require("assert");
const path = require("path");

const segment = "\ud83d\udc04";
assert.strictEqual(path.join("/tmp", segment), "/tmp/🐄");
assert.strictEqual(path.resolve("/tmp", `weird ${segment}`), "/tmp/weird 🐄");
