const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-122-${process.pid}`;
  const fd = fs.openSync(path, "w+");
  const buffer = Buffer.from("abcd");
  const written = await new Promise((resolve, reject) =>
    fs.write(
      fd,
      { buffer, offset: 1, length: 2, position: 0 },
      (error, count) => (error ? reject(error) : resolve(count))
    )
  );
  fs.closeSync(fd);
  if (written !== 2 || fs.readFileSync(path, "utf8") !== "bc")
    throw new Error("write options mismatch");
})().then(() => undefined);
