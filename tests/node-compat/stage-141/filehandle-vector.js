const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-141-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  const written = await handle.writev(
    [Buffer.from("ab"), Buffer.from("cd")],
    0,
  );
  assert.strictEqual(written.bytesWritten, 4);
  const buffers = [Buffer.alloc(2), Buffer.alloc(2)];
  const read = await handle.readv(buffers, 0);
  await handle.close();
  assert.strictEqual(read.bytesRead, 4);
  assert.strictEqual(Buffer.concat(read.buffers).toString(), "abcd");
  fs.rmSync(path);
})().then(() => undefined);
