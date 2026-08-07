const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = path.join(process.cwd(), "target", "compat", "stage-1929");
const source = path.join(root, "src");
const destination = path.join(root, "dst");
fs.mkdirSync(source, { recursive: true });
fs.writeFileSync(path.join(source, "keep.js"), "keep");
fs.writeFileSync(path.join(source, "skip.txt"), "skip");
fs.cp(
  source,
  destination,
  {
    recursive: true,
    filter: async (entry) => {
      await Promise.resolve();
      return entry.endsWith("src") || entry.endsWith("keep.js");
    },
  },
  (error) => {
    assert.strictEqual(error, null);
    assert.strictEqual(
      fs.readFileSync(path.join(destination, "keep.js"), "utf8"),
      "keep",
    );
    assert.strictEqual(
      fs.existsSync(path.join(destination, "skip.txt")),
      false,
    );
    fs.rmSync(root, { recursive: true, force: true });
    console.log("fs cp async filter passed");
  },
);
