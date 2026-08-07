const { test } = require("node:test");
test("message channel basic delivery", async () => {
  const channel = new MessageChannel();
  const received = new Promise((resolve) => {
    channel.port2.onmessage = (event) => resolve(event.data);
  });
  channel.port1.postMessage("ready");
  if ((await received) !== "ready") {
    throw new Error("message was not delivered");
  }
});
