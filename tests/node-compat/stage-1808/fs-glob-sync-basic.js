const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = fs.mkdtempSync(path.join(process.cwd(), "quench-glob-"));
fs.writeFileSync(path.join(root, "a.txt"), "a");
fs.writeFileSync(path.join(root, "b.js"), "b");
assert.deepStrictEqual(fs.globSync("*.txt", { cwd: root }), ["a.txt"]);
fs.rmSync(root, { recursive: true, force: true });
