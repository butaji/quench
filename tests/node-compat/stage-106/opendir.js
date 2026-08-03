const fs = require("fs");
const assert = require("assert");

const path = "/tmp/quench-node-stage-106";
fs.mkdirSync(path, { recursive: true });
fs.writeFileSync(`${path}/file`, "x");
const dir = fs.opendirSync(path);
assert.strictEqual(dir.readSync().isFile(), true);
assert.strictEqual(dir.readSync(), null);
dir.closeSync();
fs.rmSync(path, { recursive: true });

const callbackPath = "/tmp/quench-node-stage-106-callback";
fs.mkdirSync(callbackPath);
fs.writeFileSync(`${callbackPath}/file`, "x");
fs.opendir(callbackPath, (error, handle) => {
  assert.ifError(error);
  assert.strictEqual(handle.readSync().isFile(), true);
  handle.close((closeError) => {
    assert.ifError(closeError);
    fs.rmSync(callbackPath, { recursive: true });
  });
});

(async () => {
  const promisePath = "/tmp/quench-node-stage-106-promise";
  fs.mkdirSync(promisePath);
  fs.writeFileSync(`${promisePath}/file`, "x");
  const handle = await fs.promises.opendir(promisePath);
  const entry = await handle.read();
  assert.strictEqual(entry.isFile(), true);
  assert.strictEqual(await handle.read(), null);
  await handle.close();
  fs.rmSync(promisePath, { recursive: true });
})().then(() => undefined);
