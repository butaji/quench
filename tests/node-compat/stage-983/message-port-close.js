const { test } = require("node:test");
test("closed port drops messages", async () => {
  const channel = new MessageChannel();
  let received = false;
  channel.port2.onmessage = () => {
    received = true;
  };
  channel.port2.close();
  channel.port1.postMessage("ignored");
  await Promise.resolve();
  if (received) throw new Error("closed MessagePort received a message");
});
