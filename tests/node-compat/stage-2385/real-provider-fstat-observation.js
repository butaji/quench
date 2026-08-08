const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.join(process.cwd(), "stage-2385-fstat");
fs.mkdirSync(root, { recursive: true });
const content = "a".repeat(8192) + "trailing data";
fs.writeFileSync(path.join(root, "zero-stat.txt"), content);
const provider = vfs.create(new vfs.RealFSProvider(root));
(async () => {
  const syncHandle = await provider.provider.open("/zero-stat.txt", "r");
  const asyncHandle = await provider.provider.open("/zero-stat.txt", "r");

  let syncCalls = 0;
  let asyncCalls = 0;
  const originalFstatSync = fs.fstatSync;
  const originalFstat = fs.fstat;
  fs.fstatSync = (...args) => {
    syncCalls++;
    const stats = originalFstatSync(...args);
    stats.size = 0;
    return stats;
  };
  fs.fstat = (fd, options, callback) => {
    asyncCalls++;
    if (typeof options === "function") callback = options;
    return originalFstat(fd, options, (error, stats) => {
      if (stats) stats.size = 0;
      callback(error, stats);
    });
  };

  assert.strictEqual(syncHandle.readFileSync("utf8"), content);
  assert.strictEqual(await asyncHandle.readFile("utf8"), content);
  assert.ok(syncCalls >= 1);
  assert.ok(asyncCalls >= 1);

  fs.fstatSync = originalFstatSync;
  fs.fstat = originalFstat;
  await syncHandle.close();
  await asyncHandle.close();
})().catch((error) => {
  console.error(error);
  throw error;
});
