const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = path.join(process.cwd(), "tests/node-compat");
const dir = fs.opendirSync(root, { bufferSize: 16 });
assert.strictEqual(dir.path, root);
const first = dir.readSync();
assert(first instanceof fs.Dirent);
assert.strictEqual(typeof first.name, "string");
assert.strictEqual(first.parentPath, root);
assert.strictEqual(first.isFile() || first.isDirectory(), true);
while (dir.readSync() !== null) {}
assert.strictEqual(dir.readSync(), null);
dir.closeSync();
assert.throws(() => dir.readSync(), { code: "ERR_DIR_CLOSED" });
assert.throws(() => fs.opendirSync(__filename), /ENOTDIR/);
assert.throws(() => fs.opendirSync(root, { bufferSize: 0 }), {
  code: "ERR_OUT_OF_RANGE",
});

(async () => {
  const asyncDir = await fs.promises.opendir(root);
  const names = [];
  for await (const entry of asyncDir) names.push(entry.name);
  assert(names.length > 0);
  await assert.rejects(asyncDir.read(), { code: "ERR_DIR_CLOSED" });
})();
