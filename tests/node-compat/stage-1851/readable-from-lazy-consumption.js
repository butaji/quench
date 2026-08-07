const assert = require("assert");
const fs = require("fs");
const path = require("path");
const { Readable } = require("stream");

const file = path.join("/tmp", `quench-readable-from-${process.pid}`);
const source = Readable.from(["a", "b", "c"]);

(async () => {
  await Promise.resolve();
  await fs.promises.appendFile(file, source);
  assert.strictEqual(fs.readFileSync(file, "utf8"), "abc");
  fs.unlinkSync(file);
  console.log("Readable.from lazy consumption passed");
})();
