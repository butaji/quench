const assert = require("assert");
const { Duplex } = require("stream");

let writes = 0;
const data = Buffer.from("hello");
const duplex = Duplex({
  read() {
    this.push(data);
    this.push(null);
  },
  write(chunk, _encoding, callback) {
    writes++;
    assert.deepStrictEqual(chunk, Buffer.from("world"));
    callback();
  }
});
const { readable, writable } = Duplex.toWeb(duplex, { readableType: "bytes" });
writable.getWriter().write(Buffer.from("world"));
readable
  .getReader({ mode: "byob" })
  .read(new Uint8Array(5))
  .then(() => {
    setTimeout(() => {
      assert.strictEqual(writes, 1);
      console.log("stream duplex write after read end pass");
    }, 0);
  });
