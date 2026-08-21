const assert = require("assert");
const net = require("net");

let serverConnections = 0;
const server = net.createServer((socket) => {
  serverConnections++;
  socket.end("ok");
});
server.listen(0, () => {
  const client = new net.Socket();
  client.on("data", (chunk) => {
    assert.strictEqual(chunk.toString(), "ok");
  });
  client.on("end", () => {
    assert.strictEqual(serverConnections, 1);
    server.close(() => console.log("direct socket connect passed"));
  });
  assert.strictEqual(client.connect(server.address().port), client);
});
