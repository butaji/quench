const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = `/tmp/quench-node-stage-100-${process.pid}`;
const file = path.join(root, "file");
const directory = path.join(root, "directory");
fs.mkdirSync(root);
fs.writeFileSync(file, "x");
fs.mkdirSync(directory);

const check = (entries) => {
  const byName = Object.fromEntries(
    entries.map((entry) => [entry.name, entry]),
  );
  assert.strictEqual(typeof byName.file.isFile, "function");
  assert.strictEqual(byName.file.isFile(), true);
  assert.strictEqual(byName.file.isDirectory(), false);
  assert.strictEqual(byName.directory.isDirectory(), true);
  assert.strictEqual(byName.directory.isFile(), false);
  assert.strictEqual(byName.file.parentPath, root);
};

check(fs.readdirSync(root, { withFileTypes: true }));
fs.readdir(root, { withFileTypes: true }, (error, entries) => {
  assert.ifError(error);
  check(entries);
});
fs.promises
  .readdir(root, { withFileTypes: true })
  .then(check)
  .then(() => fs.rmSync(root, { recursive: true }));

console.log("readdir dirents passed");
