const assert = require("assert");
const http = require("http");

const server = http.createServer((_request, response) => {
  response.setHeader("Content-Length", 11);
  response.statusCode = 304;
  response.end();
});

server.listen(0, () => {
  const request = http.request({ port: server.address().port });
  request.on("response", (response) => {
    response.on("data", () => {
      throw new Error("304 response contained a body");
    });
    response.on("end", () => {
      assert.strictEqual(response.statusCode, 304);
      assert.strictEqual(response.headers["content-length"], "11");
      server.close();
    });
  });
  request.end();
});
