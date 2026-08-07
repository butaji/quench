const assert = require("assert");
const fs = require("fs");
const fsp = require("fs/promises");

(async () => {
  const path = "stage-2002-open.txt";
  fs.writeFileSync(path, "ok");
  const handle = await fsp.open(path);
  assert.strictEqual(typeof handle[Symbol.asyncDispose], "function");
  await handle[Symbol.asyncDispose]();
  assert.strictEqual(handle.fd, -1);
  fs.unlinkSync(path);
  console.log("fs promises open disposal passed");
})().catch((error) => {
  console.error(error);
  throw error;
});
