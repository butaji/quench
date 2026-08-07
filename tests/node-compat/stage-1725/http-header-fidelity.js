const assert = require("node:assert");
const http = require("node:http");

const server = http.createServer((request, response) => {
  delete request.headers.host;
  assert.deepStrictEqual(request.headers, {
    __proto__: null,
    connection: "keep-alive",
    "transfer-encoding": "chunked",
  });
  response.removeHeader("Date");
  response.setHeader("Keep-Alive", "timeout=1");
  response.write("hello");
  response.end("world");
  assert.deepStrictEqual(response.headers, {
    __proto__: null,
    connection: "keep-alive",
    "keep-alive": "timeout=1",
    "transfer-encoding": "chunked",
  });
  server.close();
});

server.listen(43215, function () {
  const request = http.request({
    port: this.address().port,
    method: "POST",
    path: "/",
  });
  request.write("hello");
  request.end("world");
});
