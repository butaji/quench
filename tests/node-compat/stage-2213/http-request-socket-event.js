const assert = require("assert");
const http = require("http");

const server = http.createServer((_request, response) => response.end());

server.listen(0, () => {
  const request = http.get({ port: server.address().port }, (response) => {
    response.on("end", () => server.close());
    response.resume();
  });
  request.on("socket", (socket) => {
    assert.strictEqual(typeof socket.listenerCount, "function");
  });
});
