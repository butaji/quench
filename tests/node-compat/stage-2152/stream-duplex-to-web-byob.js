const assert = require("assert");
const { Duplex } = require("stream");

const dataToRead = Buffer.from("hello");
const dataToWrite = Buffer.from("world");
const duplex = Duplex({
  read() {
    this.push(dataToRead);
    this.push(null);
  },
  write(chunk, _encoding, callback) {
    assert.deepStrictEqual(chunk, dataToWrite);
    callback();
  }
});
const { readable, writable } = Duplex.toWeb(duplex, { readableType: "bytes" });
writable.getWriter().write(dataToWrite);
readable
  .getReader({ mode: "byob" })
  .read(new Uint8Array(dataToRead.length))
  .then((result) => {
    assert.deepStrictEqual(Buffer.from(result.value), dataToRead);
    console.log("stream duplex to web byob pass");
  });
