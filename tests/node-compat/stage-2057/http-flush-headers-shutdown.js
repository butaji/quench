const assert = require("assert");
const http = require("http");

const server = http.createServer((req, res) => {
  assert.strictEqual(req.headers.foo, "bar");
  res.end("ok");
  server.close();
});
server.listen(0, "127.0.0.1", () => {
  const req = http.request({ host: "127.0.0.1", port: server.address().port });
  req.setHeader("foo", "bar");
  assert.strictEqual(req.flushHeaders(), req);
});
