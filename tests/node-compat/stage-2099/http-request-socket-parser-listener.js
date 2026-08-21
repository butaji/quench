const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.strictEqual(request.socket.listenerCount("data"), 1);
  response.end("ok");
});

server.listen(0, () => {
  const request = http.request({ port: server.address().port });
  request.on("response", (response) => {
    response.resume();
    response.on("close", () => server.close());
  });
  request.end();
});
