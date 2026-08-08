const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.on("message", (message) => {
  assert.strictEqual(message.toString(), "foobarbaz");
  socket.close();
  console.log("dgram connected string array passed");
});
socket.bind(0, () => {
  socket.connect(socket.address().port, () =>
    socket.send(["foo", "bar", "baz"])
  );
});
