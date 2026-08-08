const assert = require("assert");
const vfs = require("node:vfs");
const { Readable } = require("stream");
const { pipeline } = require("stream/promises");

Promise.all([
  (async () => {
    const filesystem = vfs.create();
    await pipeline(
      Readable.from([Buffer.from("hello"), Buffer.from(" world")]),
      filesystem.createWriteStream("/out.txt")
    );
    assert.strictEqual(
      filesystem.readFileSync("/out.txt", "utf8"),
      "hello world"
    );
  })(),
  (async () => {
    const filesystem = vfs.create();
    filesystem.writeFileSync("/in.txt", "hello world");
    await pipeline(
      filesystem.createReadStream("/in.txt"),
      filesystem.createWriteStream("/copied.txt")
    );
    assert.strictEqual(
      filesystem.readFileSync("/copied.txt", "utf8"),
      "hello world"
    );
  })(),
  (async () => {
    const filesystem = vfs.create();
    filesystem.writeFileSync("/pad.txt", "AAAAAAAAAA");
    await pipeline(
      Readable.from([Buffer.from("XX")]),
      filesystem.createWriteStream("/pad.txt", { start: 3, flags: "r+" })
    );
    assert.strictEqual(
      filesystem.readFileSync("/pad.txt", "utf8"),
      "AAAXXAAAAA"
    );
  })()
]).then(() => console.log("VFS concurrent pipelines passed"));
