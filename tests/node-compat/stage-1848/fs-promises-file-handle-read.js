const fs = require("fs");
const path = require("path");

const file = path.join("/tmp", `quench-file-handle-read-${process.pid}.txt`);
fs.writeFileSync(file, "abc");

(async () => {
  const handle = await fs.promises.open(file, "r");
  let closeCalls = 0;
  handle.on("close", () => closeCalls++);
  const buffer = Buffer.alloc(3);
  const result = await handle.read(buffer, 0, buffer.length, 0);
  if (result.bytesRead !== 3 || buffer.toString() !== "abc") {
    throw new Error("FileHandle.read returned the wrong data");
  }
  await handle.close();
  if (closeCalls !== 1) throw new Error("FileHandle close event mismatch");

  const streamHandle = await fs.promises.open(file, "r");
  const stream = fs.createReadStream(null, { fd: streamHandle });
  let streamed = "";
  for await (const chunk of stream) streamed += chunk.toString();
  if (streamed !== "abc") throw new Error("FileHandle stream read mismatch");
  await streamHandle.close();
  fs.unlinkSync(file);
  console.log("fs promises FileHandle read lifecycle passed");
})();
