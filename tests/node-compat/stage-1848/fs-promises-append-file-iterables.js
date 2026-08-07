const fs = require("fs");
const path = require("path");
const { Readable } = require("stream");

const file = path.join("/tmp", `quench-append-iterables-${process.pid}.txt`);

(async () => {
  await fs.promises.appendFile(file, Readable.from(["a", "b"]));
  await fs.promises.appendFile(file, {
    *[Symbol.iterator]() {
      yield "c";
    },
  });
  if (fs.readFileSync(file, "utf8") !== "abc") {
    throw new Error("appendFile did not consume stream and iterable data");
  }
  fs.unlinkSync(file);
  console.log("fs promises appendFile iterables passed");
})();
