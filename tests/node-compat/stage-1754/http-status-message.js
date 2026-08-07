const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  response.writeHead(500);
  assert.strictEqual(response.statusMessage, "Internal Server Error");
  response.end();
});

server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    assert.strictEqual(response.statusMessage, "Internal Server Error");
    response.resume();
    response.on("end", () => server.close());
  });
});
