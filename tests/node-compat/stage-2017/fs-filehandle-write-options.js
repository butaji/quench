const assert = require("assert");
const fs = require("fs");

(async () => {
  const path = "stage-2017-write.txt";
  const handle = await fs.promises.open(path, "w+");
  const buffer = Buffer.from("zyx");
  const result = await handle.write(buffer, { length: 1 });
  assert.strictEqual(result.bytesWritten, 1);
  await handle.close();
  const reader = await fs.promises.open(path, "r");
  const readBuffer = Buffer.alloc(3);
  const readResult = await reader.read(readBuffer, { length: 1 });
  assert.strictEqual(readResult.bytesRead, 1);
  assert.strictEqual(readResult.buffer, readBuffer);
  await reader.close();
  fs.unlinkSync(path);
  console.log("filehandle named write options passed");
})().catch((error) => {
  throw error;
});
