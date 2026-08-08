const assert = require("assert");
const http = require("http");

const server = http.createServer((_request, response) => {
  response.end("hello world");
});

server.listen(0, "127.0.0.1", () => {
  const request = new http.ClientRequest(server.address(), (response) => {
    let body = "";
    response.setEncoding("utf8");
    response.on("data", (chunk) => {
      body += chunk;
    });
    response.on("end", () => {
      assert.strictEqual(body, "hello world");
      server.close();
    });
  });
  request.end();
});
