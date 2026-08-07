const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  response.setHeader("content-length", [1, 2]);
  response.end("x");
});
server.listen(0, () => {
  http.get({ port: server.address().port }, () => {
    assert.fail("duplicate content-length was accepted");
  }).on("error", (error) => {
    assert.strictEqual(error.code, "HPE_UNEXPECTED_CONTENT_LENGTH");
    server.close();
  });
});
