const assert = require("node:assert");
const fs = require("node:fs");

const dirent = new fs.Dirent("unknown", 0);
assert.strictEqual(dirent.isFile(), true);
assert.strictEqual(dirent.isDirectory(), false);
console.log("readdir unknown dirent passed");
