const assert = require("assert");
const http = require("http");
const url = require("url");

const path = "/foo?bar";
let requests = 0;
const server = http.createServer((request, response) => {
  requests++;
  assert.strictEqual(request.url, path);
  response.end("ok");
});

server.listen(0, () => {
  const target = `http://localhost:${server.address().port}${path}`;
  http.get(target, () => {
    http.get(url.parse(target), () => {
      http.get(new URL(target), () => {
        assert.strictEqual(requests, 3);
        server.close();
      });
    });
  });
});
