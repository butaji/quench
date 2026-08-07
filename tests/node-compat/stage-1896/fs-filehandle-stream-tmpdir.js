const assert = require("assert");
const fs = require("fs");
const path = require("path");
const tmpdir = require("../../node/test/common/tmpdir");
const { finished } = require("stream/promises");
const { buffer } = require("stream/consumers");

(async () => {
  tmpdir.refresh();
  const writePath = path.resolve(tmpdir.path, "tmp-write.txt");
  const writer = await fs.promises.open(writePath, "w");
  const data = Buffer.from("Hello world".repeat(100));
  const writeStream = writer.createWriteStream();
  writeStream.end(data);
  await finished(writeStream);
  assert.deepStrictEqual(fs.readFileSync(writePath), data);
  await writer.close();

  const readPath = path.resolve(tmpdir.path, "tmp-read.txt");
  fs.writeFileSync(readPath, data);
  const reader = await fs.promises.open(readPath);
  assert.deepStrictEqual(await buffer(reader.createReadStream()), data);
  await reader.close();
})();
