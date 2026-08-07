const assert = require("assert");
const fs = require("fs");

(async () => {
  const path = "stage-2024-write.txt";
  const buffer = Buffer.from("zyx");
  const fh = await fs.promises.open(path, "w");
  const writeResult = await fh.write(buffer, { length: 1 });
  const writeBufCopy = Uint8Array.prototype.slice.call(writeResult.buffer);
  await fh.close();
  const reader = await fs.promises.open(path, "r");
  const readResult = await reader.read(buffer, { length: 1 });
  const readBufCopy = Uint8Array.prototype.slice.call(readResult.buffer);
  await reader.close();
  assert(writeResult.bytesWritten >= readResult.bytesRead);
  assert.strictEqual(writeResult.bytesWritten, 1);
  assert.strictEqual(readResult.bytesRead, 1);
  assert.deepStrictEqual(writeBufCopy, readBufCopy);
  assert.deepStrictEqual(writeResult.buffer, readResult.buffer);
  fs.unlinkSync(path);
  console.log("filehandle result shape passed");
})().catch((error) => {
  throw error;
});
