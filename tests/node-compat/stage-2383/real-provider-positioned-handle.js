const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");
const { getVirtualFd } = require("internal/vfs/fd");

const root = path.join(process.cwd(), "stage-2383-real-handle");
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(path.join(root, "file.txt"), "hello world");

const provider = vfs.create(new vfs.RealFSProvider(root));
const fd = provider.openSync("/file.txt", "r+");
const handle = getVirtualFd(fd).entry;
const buffer = Buffer.alloc(5);

assert.strictEqual(handle.readSync(buffer, 0, 5, 0), 5);
assert.strictEqual(buffer.toString(), "hello");
assert.strictEqual(handle.writeSync(Buffer.from("J"), 0, 1, 0), 1);
assert.strictEqual(handle.readFileSync("utf8"), "Jello world");
assert.strictEqual(handle.statSync().isFile(), true);
provider.closeSync(fd);

(async () => {
  await provider.promises.writeFile("/h2.txt", "abcdef");
  const asyncHandle = await provider.provider.open("/h2.txt", "r+");
  const asyncBuffer = Buffer.alloc(3);
  assert.strictEqual(asyncHandle.readSync(asyncBuffer, 0, 3, 0), 3);
  assert.strictEqual(
    (await asyncHandle.read(Buffer.alloc(3), 0, 3, 3)).bytesRead,
    3
  );
  asyncHandle.writeSync(Buffer.from("ZZ"), 0, 2, 0);
  assert.strictEqual(
    (await asyncHandle.write(Buffer.from("YY"), 0, 2, 4)).bytesWritten,
    2
  );
  asyncHandle.writeFileSync("OVERWRITTEN");
  assert.strictEqual(asyncHandle.readFileSync("utf8"), "OVERWRITTEN");
  await asyncHandle.writeFile("async-overwrite");
  assert.strictEqual(await asyncHandle.readFile("utf8"), "async-overwrite");
  asyncHandle.truncateSync(3);
  await asyncHandle.truncate(2);
  await asyncHandle.close();
})();

{
  const fsModule = require("fs");
  let observed = 0;
  const originalFstatSync = fsModule.fstatSync;
  fsModule.fstatSync = (...args) => {
    observed++;
    return originalFstatSync(...args);
  };
  const observedFd = provider.openSync("/file.txt", "r");
  getVirtualFd(observedFd).entry.readFileSync("utf8");
  assert.strictEqual(observed, 1);
  provider.closeSync(observedFd);
  fsModule.fstatSync = originalFstatSync;
}
