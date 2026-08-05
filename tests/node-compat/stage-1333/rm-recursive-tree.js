const assert = require("node:assert");
const fs = require("node:fs");

const root = "rm-recursive-tree";
fs.mkdirSync(`${root}/nested`, { recursive: true });
fs.writeFileSync(`${root}/nested/file.txt`, "data");
fs.symlinkSync("nested/file.txt", `${root}/link`);
fs.rmSync(root, { recursive: true, force: true });
assert.strictEqual(fs.existsSync(root), false);
console.log("rm recursive tree passed");
