const assert = require("assert");
const dgram = require("dgram");
const dns = require("dns");

dns.lookup = () => {
  throw new Error("literal IP lookup must not call dns.lookup");
};

const receiver = dgram.createSocket("udp4");
const sender = dgram.createSocket("udp4");
receiver.on("message", (message) => {
  assert.strictEqual(message.toString(), "payload");
  receiver.close();
  sender.close();
  console.log("dgram literal IP lookup passed");
});
receiver.bind(0, "127.0.0.1", () => {
  sender.send("payload", receiver.address().port, "127.0.0.1", (error) => {
    assert.ifError(error);
  });
});
