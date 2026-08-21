const assert = require("assert");
const http = require("http");

const server = http.createServer((_request, response) => {
  assert.strictEqual(response.closed, false);
  response.end();
  response.destroy();
  response.end("ignored");
  assert.strictEqual(response.errored, undefined);
});

server.listen(0, () => {
  http
    .request({ port: server.address().port }, (response) => {
      response.resume().on("end", () => server.close());
    })
    .end();
});
