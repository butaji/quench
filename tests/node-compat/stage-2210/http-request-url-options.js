const assert = require("assert");
const http = require("http");

const headers = { foo: "Bar" };
const server = http.createServer((request, response) => {
  assert.strictEqual(request.url, "/ping?q=term");
  assert.strictEqual(request.headers.foo, "Bar");
  request.resume();
  request.on("end", () => response.end("pong"));
});

server.listen(0, () => {
  const url = new URL(`http://127.0.0.1:${server.address().port}/ping?q=term`);
  url.headers = headers;
  const request = http.request(url);
  request.on("close", () => server.close());
  request.end();
});
