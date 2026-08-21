const assert = require("assert");
const { Duplex, PassThrough, Readable } = require("stream");

const through = new PassThrough({ objectMode: true });
let result = "";
const duplex = Readable.from(["foo", "bar"], { objectMode: true }).pipe(
  Duplex.from({ readable: through, writable: through }),
);
duplex.on("data", (data) => {
  duplex.pause();
  setImmediate(() => duplex.resume());
  result += data;
});
duplex.on("end", () => {
  assert.strictEqual(result, "foobar");
  console.log("stream duplex passthrough pass");
});
duplex.on("close", () => assert.strictEqual(result, "foobar"));
