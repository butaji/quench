const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.throws(() => response.writeHead(-1), {
    name: "RangeError",
    code: "ERR_HTTP_INVALID_STATUS_CODE",
    message: "Invalid status code: -1",
  });
  response.statusCode = 204;
  response.end();
  server.close();
});

server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => response.resume());
});

console.log("http writeHead validation ok");
