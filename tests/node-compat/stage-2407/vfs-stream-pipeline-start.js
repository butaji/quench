const assert = require("assert");
const vfs = require("node:vfs");
const { Readable } = require("stream");
const { pipeline } = require("stream/promises");

(async () => {
  const filesystem = vfs.create();
  filesystem.writeFileSync("/pad.txt", "AAAAAAAAAA");
  await pipeline(
    Readable.from([Buffer.from("XX")]),
    filesystem.createWriteStream("/pad.txt", { start: 3, flags: "r+" })
  );
  assert.strictEqual(filesystem.readFileSync("/pad.txt", "utf8"), "AAAXXAAAAA");
  console.log("VFS pipeline start passed");
})();
