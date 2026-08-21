const assert = require("assert");
const dgram = require("dgram");

const receiver = dgram.createSocket("udp4");
const address = receiver.bindSync({ address: "127.0.0.1", port: 0 });
receiver.on("message", (message) => {
  assert.strictEqual(message.toString(), "hello");
  receiver.close();
  console.log("dgram sync message passed");
});
const sender = dgram.createSocket("udp4");
sender.send("hello", address.port, "127.0.0.1", () => sender.close());
