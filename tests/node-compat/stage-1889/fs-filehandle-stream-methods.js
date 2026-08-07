const assert = require("assert");
const fs = require("fs");
const path = require("path");
const { finished } = require("stream/promises");
const { buffer } = require("stream/consumers");

(async () => {
  const writePath = path.join("/tmp", `quench-fh-write-${process.pid}`);
  const writer = await fs.promises.open(writePath, "w");
  const data = Buffer.from("Hello world".repeat(3));
  const writeStream = writer.createWriteStream();
  writeStream.end(data);
  await finished(writeStream);
  assert.deepStrictEqual(fs.readFileSync(writePath), data);
  await writer.close();

  const readPath = path.join("/tmp", `quench-fh-read-${process.pid}`);
  fs.writeFileSync(readPath, data);
  const reader = await fs.promises.open(readPath, "r");
  assert.deepStrictEqual(await buffer(reader.createReadStream()), data);
  await reader.close();
})();
