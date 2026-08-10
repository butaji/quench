const assert = require("assert");
const net = require("net");

const server = net.createServer();
const events = [];
server.once("close", () => events.push("close"));
server.listen(0, () => {
  assert.strictEqual(
    server.close(() => events.push("callback")),
    server,
  );
  queueMicrotask(() => {
    assert.deepStrictEqual(events, ["close", "callback"]);
  });
});
