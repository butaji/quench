const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-164-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.writeFile("hello");
  await handle.close();
  if (fs.readFileSync(path, "utf8") !== "hello")
    throw new Error("filehandle writeFile mismatch");
})();
