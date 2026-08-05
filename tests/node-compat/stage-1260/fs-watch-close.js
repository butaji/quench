const assert = require("node:assert");
const fs = require("node:fs");

const watcher = fs.watch(__filename, {});
assert.strictEqual(watcher.close(), watcher);
fs.watchFile(__filename, {});
fs.unwatchFile(__filename);

console.log("fs watcher close passed");
