const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-142-${process.pid}`;
  fs.writeFileSync(path, "abcdef");
  const handle = await fs.promises.open(path, "r+");
  await handle.truncate(2);
  await handle.close();
  if (fs.readFileSync(path, "utf8") !== "ab")
    throw new Error("filehandle truncate mismatch");
})().then(() => undefined);
