const fs = require("fs");
const { Buffer } = require("buffer");

(async () => {
  const path = "/tmp/quench-node-stage-95";
  await fs.promises.appendFile(path, "a");
  await fs.promises.appendFile(path, Buffer.from("b"));
  if (fs.readFileSync(path, "utf8") !== "ab") {
    throw new Error("promise append mismatch");
  }
})().then(() => undefined);
