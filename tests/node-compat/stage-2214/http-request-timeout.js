const assert = require("assert");
const http = require("http");

const server = http.createServer(() => {});

server.listen(0, () => {
  const request = http.get({ port: server.address().port, timeout: 20 });
  request.setTimeout(10);
  request.on("socket", (socket) => {
    assert.strictEqual(socket.timeout, 20);
    socket.on("connect", () => {
      assert.strictEqual(socket.timeout, 10);
      socket.setTimeout(1);
    });
  });
  request.on("timeout", () => request.destroy());
  request.on("close", () => server.close());
});
