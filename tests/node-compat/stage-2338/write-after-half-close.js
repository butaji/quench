const assert = require("assert");
const net = require("net");

const server = net.createServer((socket) => {
  socket.on("end", () => {
    socket.write("late", (error) => {
      assert.ok(error);
      server.close(() => console.log("write after half-close passed"));
    });
  });
});
server.listen(0, () => {
  const client = net.createConnection(
    server.address().port,
    () => client.end(),
  );
});
