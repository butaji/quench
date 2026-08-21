const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vfs = require("node:vfs");

(async () => {
  const root = path.resolve(`/tmp/vfs-real-access-${process.pid}`);
  fs.mkdirSync(root, { recursive: true });
  fs.writeFileSync(path.join(root, "present.txt"), "present");
  const filesystem = vfs.create(new vfs.RealFSProvider(root));
  await filesystem.promises.access("/present.txt");
  await assert.rejects(filesystem.promises.access("/missing.txt"), {
    code: "ENOENT",
  });
  console.log("real provider access passed");
})();
