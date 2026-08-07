const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.throws(() => response.setHeaders({ foo: "1" }), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  const headers = new Headers({ foo: "1" });
  response.setHeaders(headers);
  assert.strictEqual(response.getHeader("foo"), "1");
  response.writeHead(200, ["foo", "2"]);
  assert.throws(() => response.setHeaders(new Map([["bar", "3"]])), {
    code: "ERR_HTTP_HEADERS_SENT",
  });
  response.end();
});

server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    assert.strictEqual(response.headers.foo, "2");
    response.resume().on("end", () => server.close());
  });
});
