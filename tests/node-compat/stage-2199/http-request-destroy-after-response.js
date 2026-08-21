const assert = require("assert");
const http = require("http");

const server = http.createServer((_request, response) => {
  response.end("hello");
});

server.listen(0, () => {
  const request = http.get({ port: server.address().port }, (response) => {
    response.on("data", () => {
      request.destroy();
      assert.strictEqual(request.aborted, false);
      assert.strictEqual(request.destroyed, true);
      server.close();
    });
  });
  request.on(
    "error",
    () => assert.fail("destroy after response should not emit error"),
  );
});
