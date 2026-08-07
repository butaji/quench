const fs = require("fs");
const path = `/tmp/quench-node-stage-413-${process.pid}`;

fs.writeFileSync(path, "Hello World");
(async () => {
  const handle = await fs.promises.open(path, "r");
  const buffer = Buffer.alloc(5);
  await handle.read(buffer, 0, 5, null);
  if (buffer.toString() !== "Hello") throw new Error("initial read failed");
  const rest = await handle.readFile();
  if (rest.toString() !== " World") {
    throw new Error("FileHandle.readFile ignored the current position");
  }
  await handle.close();
  fs.unlinkSync(path);
  console.log("file handle read position passed");
})();
