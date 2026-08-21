const assert = require("assert");
const http = require("http");

const server = http.createServer((_request, response) =>
  response.flushHeaders()
);

server.listen(() => {
  const request = http.get({ port: server.address().port }, (response) => {
    response.on("timeout", () => {
      assert.strictEqual(response.timeout, 1);
      request.destroy();
      server.close();
    });
    response.setTimeout(1);
  });
});
