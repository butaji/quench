const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.strictEqual(request.pause(), request);
  assert.strictEqual(request.resume(), request);
  response.end("ok");
});

server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    assert.strictEqual(response.pause(), response);
    assert.strictEqual(response.resume(), response);
    response.resume();
    response.once("end", () => server.close());
  });
});
