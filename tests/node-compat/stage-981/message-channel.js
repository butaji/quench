const { test } = require("node:test");
test("message channel delivery", async () => {
  const channel = new MessageChannel();
  let received;
  channel.port2.onmessage = (event) => {
    received = event.data;
  };
  channel.port2.start();
  channel.port1.postMessage({ value: 42 });
  await Promise.resolve();
  if (received?.value !== 42) throw new Error("MessageChannel delivery failed");
});
