const assert = require("assert");
const http = require("http");

const server = http.createServer((_request, response) => {
  response.end("hello");
});

server.listen(0, () => {
  const request = http.get({ port: server.address().port }, (response) => {
    response.on("data", () => {
      request.abort();
      assert.strictEqual(request.aborted, true);
      assert.strictEqual(request.destroyed, true);
      server.close();
    });
  });
  request.on("error", () => assert.fail("abort should not emit error"));
});
