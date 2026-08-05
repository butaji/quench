const assert = require("node:assert");
const fs = require("node:fs");

const names = ["empty", "files", "for", "just", "testing"];
for (const name of names) fs.writeFileSync(`readdir-${name}`, "");
const dirents = fs.readdirSync(".", { withFileTypes: true });
for (const name of names) {
  const dirent = dirents.find((entry) => entry.name === `readdir-${name}`);
  assert(dirent instanceof fs.Dirent);
  assert.strictEqual(dirent.isFile(), true);
}
console.log("readdir dirent types passed");
