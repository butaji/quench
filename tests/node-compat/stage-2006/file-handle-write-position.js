const assert = require("assert");
const fs = require("fs");
const fsp = require("fs/promises");

(async () => {
  const path = "stage-2006-write.txt";
  const handle = await fsp.open(path, "w+");
  await handle.write(Buffer.from("Hello"), 0, 5, null);
  await handle.writeFile("World");
  await handle.close();
  assert.strictEqual(fs.readFileSync(path, "utf8"), "HelloWorld");
  fs.unlinkSync(path);
  console.log("file handle write position passed");
})().catch((error) => {
  console.error(error);
  throw error;
});
