const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  request.on("aborted", () => {
    assert.strictEqual(request.aborted, true);
    server.close();
  });
  response.write("working");
});

server.listen(0, () => {
  const request = http.get({ port: server.address().port }, (response) => {
    response.resume();
    request.abort();
  });
});
