const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-124-${process.pid}`;
  fs.writeFileSync(path, "abcd");
  const handle = await fs.promises.open(path, "r");
  const buffer = Buffer.alloc(4);
  const result = await handle.read(buffer, 0, 4, 0);
  await handle.close();
  if (result.bytesRead !== 4 || result.buffer.toString() !== "abcd")
    throw new Error("filehandle read mismatch");
})().then(() => undefined);
