const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-177-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.write("abc", 0, 3);
  if (fs.readFileSync(path, "utf8") !== "abc") {
    throw new Error("filehandle string write mismatch");
  }
  for (const value of [123, {}, null, undefined, true]) {
    try {
      await handle.write(value, 0, 1);
      throw new Error("accepted invalid filehandle write");
    } catch (error) {
      if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
    }
  }
  await handle.close();
})();
