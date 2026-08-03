const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-120-${process.pid}`;
  fs.writeFileSync(path, "abcd");
  const fd = fs.openSync(path, "r");
  const buffers = [Buffer.alloc(2), Buffer.alloc(2)];
  const result = await fs.promises.readv(fd, buffers, 0);
  if (
    result.bytesRead !== 4 ||
    Buffer.concat(result.buffers).toString() !== "abcd"
  )
    throw new Error("promise readv mismatch");
  fs.closeSync(fd);
})().then(() => undefined);
