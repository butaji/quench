const assert = require("assert");
const dgram = require("dgram");

const events = [];
const receiver = dgram.createSocket("udp4");
const sender = dgram.createSocket("udp4");

receiver.on("listening", () => events.push("listening"));
receiver.on("message", (message, remote) => {
  events.push(`message:${message.toString()}`);
  assert.strictEqual(remote.family, "IPv4");
  assert.strictEqual(remote.address, "127.0.0.1");
  receiver.close(() => {
    events.push("receiver-close-callback");
    sender.close(() => {
      events.push("sender-close-callback");
      assert.deepStrictEqual(events, [
        "listening",
        "message:ok",
        "receiver-close-callback",
        "sender-close-callback",
      ]);
      console.log("dgram bind/send/close trace passed");
    });
  });
});

receiver.bind(0, "127.0.0.1", () => {
  const address = receiver.address();
  assert.strictEqual(address.address, "127.0.0.1");
  assert.strictEqual(address.family, "IPv4");
  assert.ok(address.port > 0);
  sender.send(Buffer.from("ok"), address.port, "127.0.0.1", (error) => {
    assert.ifError(error);
  });
});
