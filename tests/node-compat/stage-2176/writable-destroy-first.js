const { Writable } = require("stream");

const first = new Writable({
  write(_chunk, _encoding, callback) {
    callback();
  },
});
first.on("close", () => {});
first.destroy();
if (!first.destroyed) throw new Error("first was not destroyed");

const second = new Writable({
  write(_chunk, _encoding, callback) {
    this.destroy(new Error("asd"));
    callback();
  },
});
second.on("error", (error) => {
  if (error.message !== "asd") throw new Error("wrong error");
});
second.end("asd");
if (!second.destroyed) throw new Error("second was not destroyed");
