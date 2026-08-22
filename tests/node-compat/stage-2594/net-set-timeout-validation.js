const assert = require("node:assert");
const net = require("node:net");

const server = net.createServer((socket) => {
  assert.strictEqual(socket.setTimeout(25), socket);
  assert.strictEqual(socket.timeout, 25);
  assert.throws(() => socket.setTimeout("25"), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  assert.throws(() => socket.setTimeout(25, "not a callback"), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  socket.destroy();
  server.close(() => console.log("net setTimeout validation passed"));
});
server.listen(0, "127.0.0.1", () => {
  const client = net.createConnection(server.address().port, "127.0.0.1");
  client.on("error", assert.fail);
});
