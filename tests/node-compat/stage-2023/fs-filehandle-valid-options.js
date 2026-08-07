const assert = require("assert");
const fs = require("fs");

(async () => {
  const path = "stage-2023-write.txt";
  const buffer = Buffer.from("zyx");
  for (
    const options of [
      undefined,
      null,
      {},
      { length: 1 },
      { position: 5 },
      { length: 1, position: 5 },
      { length: 1, position: -1, offset: 2 },
      { length: null },
      { position: null },
      { offset: 1 },
    ]
  ) {
    const writer = await fs.promises.open(path, "w");
    const writeResult = await writer.write(buffer, options);
    await writer.close();
    const reader = await fs.promises.open(path, "r");
    const readResult = await reader.read(buffer, options);
    await reader.close();
    assert(writeResult.bytesWritten >= readResult.bytesRead);
  }
  fs.unlinkSync(path);
  console.log("valid filehandle options passed");
})().catch((error) => {
  throw error;
});
