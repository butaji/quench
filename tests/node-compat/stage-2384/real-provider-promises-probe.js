const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

const root = path.join(process.cwd(), "stage-2384-real-promises");
fs.mkdirSync(root, { recursive: true });
const provider = vfs.create(new vfs.RealFSProvider(root));

(async () => {
  await provider.promises.writeFile("/a.txt", "hello");
  assert.strictEqual(
    await provider.promises.readFile("/a.txt", "utf8"),
    "hello"
  );
  const stat = await provider.promises.stat("/a.txt");
  assert.strictEqual(stat.size, 5);
  assert.strictEqual((await provider.promises.lstat("/a.txt")).isFile(), true);
  await provider.promises.access("/a.txt");
  await assert.rejects(provider.promises.access("/missing.txt"), {
    code: "ENOENT"
  });
  await provider.promises.mkdir("/d/sub", { recursive: true });
  assert.deepStrictEqual((await provider.promises.readdir("/d")).sort(), [
    "sub"
  ]);
  await provider.promises.rmdir("/d/sub");
  await provider.promises.writeFile("/old.txt", "x");
  await provider.promises.rename("/old.txt", "/new.txt");
  assert.strictEqual(provider.existsSync("/old.txt"), false);
  assert.strictEqual(provider.existsSync("/new.txt"), true);
  await provider.promises.unlink("/new.txt");
  await provider.promises.copyFile("/a.txt", "/copy.txt");
  assert.strictEqual(
    await provider.promises.readFile("/copy.txt", "utf8"),
    "hello"
  );
  await assert.rejects(provider.provider.open("/missing.txt", "r"), {
    code: "ENOENT"
  });
})().catch((error) => {
  console.error(error);
  throw error;
});
