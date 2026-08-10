const assert = require("assert");
const { Duplex } = require("stream");

let read;
let written;
const duplex = Duplex({
  objectMode: true,
  read() {},
  write(chunk, _encoding, callback) {
    written = chunk;
    callback();
  },
});
duplex.on("data", (chunk) => {
  read = chunk;
});
duplex.push({ val: 1 });
duplex.end({ val: 2 });
setTimeout(() => {
  assert.deepStrictEqual(read, { val: 1 });
  assert.deepStrictEqual(written, { val: 2 });
  console.log("stream duplex side isolation pass");
}, 0);
