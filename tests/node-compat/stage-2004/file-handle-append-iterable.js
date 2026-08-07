const assert = require("assert");
const fs = require("fs");
const fsp = require("fs/promises");

(async () => {
  const path = "stage-2004-append.txt";
  const handle = await fsp.open(path, "a");
  await handle.appendFile(["a", "b", "c"]);
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "utf8"), "abc");
  fs.unlinkSync(path);
  console.log("file handle append iterable passed");
})().catch((error) => {
  console.error(error);
  throw error;
});
