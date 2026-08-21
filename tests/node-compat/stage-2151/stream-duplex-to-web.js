const assert = require("assert");
const { Duplex } = require("stream");

const duplex = Duplex({
  read() {
    this.push(Buffer.from("hello"));
    this.push(null);
  },
  write(chunk, _encoding, callback) {
    assert.deepStrictEqual(chunk, Buffer.from("world"));
    callback();
  },
});
const { readable, writable } = Duplex.toWeb(duplex);
writable.getWriter().write(Buffer.from("world"));
readable
  .getReader()
  .read()
  .then((result) => {
    assert.deepStrictEqual(Buffer.from(result.value), Buffer.from("hello"));
    console.log("stream duplex to web pass");
  });
