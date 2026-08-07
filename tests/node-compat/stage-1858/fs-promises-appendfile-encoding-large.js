const assert = require("assert");
const fs = require("fs");
const path = require("path");
const { Readable } = require("stream");

(async () => {
  const latin = path.join("/tmp", `quench-latin-${process.pid}`);
  await fs.promises.appendFile(
    latin,
    Readable.from(["ümlaut", " ", "sechzig"]),
    "latin1",
  );
  assert.strictEqual(fs.readFileSync(latin, "latin1"), "ümlaut sechzig");
  fs.unlinkSync(latin);

  const large = path.join("/tmp", `quench-large-${process.pid}`);
  const expected = "dogs running".repeat(512 * 1024);
  await fs.promises.appendFile(large, {
    *[Symbol.iterator]() {
      yield Buffer.from(expected);
    },
  });
  assert.strictEqual(fs.statSync(large).size, Buffer.byteLength(expected));
  fs.unlinkSync(large);
  console.log("fs promises appendFile encoding and large iterable passed");
})();
