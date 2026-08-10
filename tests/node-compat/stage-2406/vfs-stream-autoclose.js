const assert = require("assert");
const vfs = require("node:vfs");

const filesystem = vfs.create();
filesystem.writeFileSync("/no-auto-close.txt", "content");
const stream = filesystem.createReadStream("/no-auto-close.txt", {
  autoClose: false,
});

let ended = false;
stream.on("end", () => {
  ended = true;
  setImmediate(() => stream.destroy());
});
stream.on("close", () => {
  assert.strictEqual(ended, true);
  console.log("VFS autoClose false lifecycle passed");
});
stream.resume();
