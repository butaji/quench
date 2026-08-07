const assert = require("assert");
const fs = require("fs");
const path = require("path");
const tmpdir = require("../../node/test/common/tmpdir");
const { finished } = require("stream/promises");
const { buffer } = require("stream/consumers");

tmpdir.refresh();
const data = Buffer.from("Hello world".repeat(100));
async function validateWrite() {
  const file = path.resolve(tmpdir.path, "tmp-write.txt");
  const handle = await fs.promises.open(file, "w");
  const stream = handle.createWriteStream();
  stream.end(data);
  await finished(stream);
  assert.deepStrictEqual(fs.readFileSync(file), data);
  await handle.close();
}
async function validateRead() {
  const file = path.resolve(tmpdir.path, "tmp-read.txt");
  fs.writeFileSync(file, data);
  const handle = await fs.promises.open(file);
  assert.deepStrictEqual(await buffer(handle.createReadStream()), data);
  await handle.close();
}
Promise.all([validateWrite(), validateRead()]);
