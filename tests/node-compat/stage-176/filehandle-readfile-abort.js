const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-176-${process.pid}`;
  fs.writeFileSync(path, "read me");
  const handle = await fs.promises.open(path, "r");
  if ((await handle.readFile({ encoding: "utf8" })) !== "read me") {
    throw new Error("filehandle readFile mismatch");
  }
  const controller = new AbortController();
  controller.abort();
  try {
    await handle.readFile({ signal: controller.signal });
    throw new Error("readFile abort accepted");
  } catch (error) {
    if (error.name !== "AbortError") throw error;
  }
  await handle.close();
})();
