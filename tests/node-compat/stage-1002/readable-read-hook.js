const { Readable } = require("stream");
const { test } = require("node:test");
test("Readable invokes _read when data flow starts", async () => {
  let called = false;
  const readable = new Readable();
  readable._read = () => {
    called = true;
    readable.push("value");
    readable.push(null);
  };
  await new Promise((resolve) =>
    readable.on("data", (value) => {
      if (value.toString() !== "value") throw new Error("chunk was incorrect");
      resolve();
    })
  );
  if (!called) throw new Error("_read was not invoked");
});
