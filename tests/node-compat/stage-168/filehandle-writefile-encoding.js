const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-168-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.writeFile(["ümlaut", " sechzig"], "latin1");
  await handle.close();
  if (fs.readFileSync(path, "latin1") !== "ümlaut sechzig")
    throw new Error("filehandle encoding mismatch");
})();
