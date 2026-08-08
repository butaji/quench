const assert = require("assert");
const { Duplex } = require("stream");

let writes = 0;
const duplex = Duplex({
  read() {
    this.push(Buffer.from("hello"));
    this.push(null);
  },
  write(chunk, _encoding, callback) {
    writes++;
    assert.deepStrictEqual(chunk, Buffer.from("world"));
    callback();
  }
});
const first = Duplex.toWeb(duplex);
first.writable.getWriter().write(Buffer.from("world"));
first.readable
  .getReader()
  .read()
  .then(() => {
    const second = Duplex.toWeb(duplex, { readableType: "bytes" });
    second.readable.getReader({ mode: "byob" }).read(new Uint8Array(5));
    setTimeout(() => {
      assert.strictEqual(writes, 1);
      console.log("stream duplex to web repeat pass");
    }, 0);
  });
