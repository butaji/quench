const assert = require("assert");
const fs = require("node:fs");
const path = require("path");
const folder = fs.mkdtempSync(path.join("/tmp", "quench-node-"));
const file = path.join(folder, "round-trip.txt");
fs.writeFileSync(file, "hello from Node compatibility");
assert.strictEqual(
  fs.readFileSync(file, "utf8"),
  "hello from Node compatibility",
);
assert.strictEqual(fs.statSync(file).isFile(), true);
