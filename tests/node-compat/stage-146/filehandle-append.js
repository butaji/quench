const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-146-${process.pid}`;
  fs.writeFileSync(path, "a");
  const handle = await fs.promises.open(path, "a");
  await handle.appendFile("b");
  await handle.close();
  if (fs.readFileSync(path, "utf8") !== "ab")
    throw new Error("filehandle append mismatch");
})().then(() => undefined);
