const fs = require("fs");
const path = `/tmp/quench-node-stage-414-${process.pid}`;

fs.writeFileSync(path, "Hello World");
(async () => {
  const handle = await fs.promises.open(path, "r");
  const buffer = Buffer.alloc(6);
  await handle.read(buffer, 0, 6, null);
  const rest = await handle.readFile("utf8");
  if (rest !== "World") {
    throw new Error("FileHandle.readFile ignored its encoding");
  }
  await handle.close();
  fs.unlinkSync(path);
  console.log("file handle read encoding passed");
})();
