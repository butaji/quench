const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-140-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  const result = await handle.write(Buffer.from("abcd"), 1, 2, 0);
  await handle.close();
  if (result.bytesWritten !== 2 || fs.readFileSync(path, "utf8") !== "bc")
    throw new Error("filehandle write mismatch");
})().then(() => undefined);
