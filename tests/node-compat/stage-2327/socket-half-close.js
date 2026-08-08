const assert = require("assert");
const net = require("net");

const server = net.createServer((socket) => {
  socket.on("end", () => {
    socket.end("response");
  });
});
server.listen(0, () => {
  const client = net.createConnection(server.address().port, () => {
    client.end("request");
    assert.strictEqual(client.readyState, "readOnly");
  });
  client.on("data", (chunk) => {
    assert.strictEqual(chunk.toString(), "response");
    client.destroy();
    server.close(() => console.log("socket half-close passed"));
  });
});
