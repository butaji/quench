const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const root = path.join(process.cwd(), "tests/node/test/.tmp.0/cp-tree");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(path.join(root, "src", "nested"), { recursive: true });
fs.writeFileSync(path.join(root, "src", "keep.txt"), "keep");
fs.writeFileSync(path.join(root, "src", "skip.txt"), "skip");
fs.writeFileSync(path.join(root, "src", "nested", "deep.txt"), "deep");

fs.cpSync(path.join(root, "src"), path.join(root, "dest"), {
  recursive: true,
  filter: (source) => !source.endsWith("skip.txt"),
});

assert.strictEqual(
  fs.readFileSync(path.join(root, "dest", "keep.txt"), "utf8"),
  "keep",
);
assert.strictEqual(fs.existsSync(path.join(root, "dest", "skip.txt")), false);
assert.strictEqual(
  fs.readFileSync(path.join(root, "dest", "nested", "deep.txt"), "utf8"),
  "deep",
);
console.log("fs cp recursive filter passed");
