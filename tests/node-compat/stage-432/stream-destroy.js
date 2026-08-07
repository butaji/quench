const { Readable, Writable } = require("stream");

for (const stream of [Readable.from(["value"]), new Writable()]) {
  if (stream.destroyed !== false) throw new Error("stream started destroyed");
  let error;
  stream.on("error", (caught) => {
    error = caught;
  });
  stream.on("close", () => {
    if (!stream.destroyed) throw new Error("destroy did not set destroyed");
    if (!error || error.message !== "cancelled") {
      throw new Error("destroy did not emit its error");
    }
    console.log("stream destroy passed");
  });
  stream.destroy(new Error("cancelled"));
}
