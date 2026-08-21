const assert = require("assert");
const net = require("net");

const server = net.createServer((socket) => {
  socket.write("one");
  socket.write("two");
  socket.end("three");
});

server.listen(0, () => {
  const client = net.connect(server.address().port);
  const chunks = [];
  client.on("data", (chunk) => {
    chunks.push(chunk.toString());
    client.pause();
    setTimeout(() => client.resume(), 1);
  });
  client.on("end", () => {
    assert.deepStrictEqual(chunks, ["one", "two", "three"]);
    server.close(() => console.log("net pause queue passed"));
  });
});
