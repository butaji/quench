const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const file = path.join("/tmp", `quench-readstream-${process.pid}`);
  fs.writeFileSync(file, "Hello world");
  const handle = await fs.promises.open(file, "r");
  const stream = fs.createReadStream(null, { fd: handle });
  const chunks = [];
  for await (const chunk of stream) chunks.push(Buffer.from(chunk));
  assert.strictEqual(Buffer.concat(chunks).toString(), "Hello world");
  await handle.close();
})();
