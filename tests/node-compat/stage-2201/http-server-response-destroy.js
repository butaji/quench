const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.strictEqual(response.closed, false);
  request.pipe(response);
  response.on("error", () => assert.fail("destroy should not emit error"));
  response.on("close", () => {
    assert.strictEqual(response.closed, true);
    response.end("after close");
    server.close();
  });
});

server.listen(0, () => {
  http
    .request({ port: server.address().port, method: "PUT" })
    .on("response", (response) => response.destroy())
    .end("input");
});
