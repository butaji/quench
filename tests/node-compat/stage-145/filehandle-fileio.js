const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-145-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.writeFile("hello");
  if ((await handle.readFile("utf8")) !== "hello")
    throw new Error("filehandle readFile mismatch");
  await handle.close();
})().then(() => undefined);
