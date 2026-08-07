const fs = require("fs");
const { Buffer } = require("buffer");

(async () => {
  const path = "/tmp/quench-node-stage-82";
  await fs.promises.writeFile(path, Buffer.from([1, 2, 255]));
  const value = await fs.promises.readFile(path);
  if (!Buffer.isBuffer(value) || value.length !== 3 || value[2] !== 255) {
    throw new Error("promise binary IO mismatch");
  }
})().then(() => undefined);
