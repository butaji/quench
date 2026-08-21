const assert = require("assert");
const fs = require("fs");
const vfs = require("node:vfs");

const root = `${process.cwd()}/tests/node-compat/stage-2382/root`;
fs.mkdirSync(root, { recursive: true });
fs.writeFileSync(`${root}/file.txt`, "hello");
const provider = vfs.create(new vfs.RealFSProvider(root));

(async () => {
  const handle = await provider.provider.open("/file.txt", "r");
  assert.strictEqual(handle.readFileSync("utf8"), "hello");
  assert.strictEqual(await handle.readFile("utf8"), "hello");
  await handle.close();
  assert.strictEqual(handle.closed, true);
  console.log("real provider handle read passed");
})();
