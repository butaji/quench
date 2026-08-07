const assert = require("node:assert");
const fs = require("node:fs");

function listener() {}
const watcher = fs.watchFile(__filename, listener);
assert.strictEqual(typeof watcher.ref, "function");
assert.strictEqual(typeof watcher.unref, "function");
assert.strictEqual(typeof watcher.hasRef, "function");
assert.strictEqual(watcher.hasRef(), true);
assert.strictEqual(watcher.unref(), watcher);
assert.strictEqual(watcher.hasRef(), false);
assert.strictEqual(watcher.ref(), watcher);
assert.strictEqual(watcher.hasRef(), true);
fs.unwatchFile(__filename, listener);
console.log("fs watchFile lifecycle passed");
