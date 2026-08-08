const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
for (const [offset, length, message] of [
  [6, 0, '"offset" is outside of buffer bounds'],
  [0, 6, '"length" is outside of buffer bounds'],
  [3, 4, '"length" is outside of buffer bounds']
]) {
  assert.throws(() => socket.send("hello", offset, length), {
    code: "ERR_BUFFER_OUT_OF_BOUNDS",
    message
  });
}
socket.close();
console.log("dgram send offset-length passed");
