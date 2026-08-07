const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = "/tmp/quench-node-stage-105";
  fs.mkdirSync(path, { recursive: true });
  fs.writeFileSync(`${path}/file`, "x");
  fs.mkdirSync(`${path}/directory`);
  const entries = await fs.promises.readdir(path, { withFileTypes: true });
  const byName = Object.fromEntries(
    entries.map((entry) => [entry.name, entry]),
  );
  assert.strictEqual(byName.file.isFile(), true);
  assert.strictEqual(byName.file.isDirectory(), false);
  assert.strictEqual(byName.directory.isDirectory(), true);
  assert.strictEqual(byName.directory.isFile(), false);
  assert.strictEqual(byName.file.parentPath, path);
  fs.rmSync(path, { recursive: true });
})().then(() => undefined);
