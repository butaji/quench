const assert = require("assert");
const net = require("net");

const server = net.createServer((socket) => {
  socket.on("data", (chunk) => {
    assert.strictEqual(chunk.toString(), "ping");
    socket.end("pong");
  });
});
server.listen(0, () => {
  const client = net.createConnection(server.address().port, () => {
    client.once("data", (chunk) => {
      assert.strictEqual(chunk.toString(), "pong");
      server.close(() => console.log("in-memory socket pair passed"));
    });
    client.write("ping");
  });
});
