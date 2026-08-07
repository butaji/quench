const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.fileURLToPath("file:///foo%5Cbar", { windows: false }),
  "/foo\\bar",
);
assert.throws(() => url.fileURLToPath("file:///foo%5Cbar", { windows: true }));
assert.throws(() => url.fileURLToPath("file:///foo%2Fbar", { windows: false }));
console.log("POSIX file path encoding matrix passed");
