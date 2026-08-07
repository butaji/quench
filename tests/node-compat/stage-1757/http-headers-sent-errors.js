const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  response.removeHeader("before");
  response.write("x");
  assert.throws(() => response.removeHeader("after"), {
    code: "ERR_HTTP_HEADERS_SENT",
    message: "Cannot remove headers after they are sent to the client",
  });
  assert.throws(() => response.setHeader("after", "x"), {
    code: "ERR_HTTP_HEADERS_SENT",
  });
  response.end();
});

server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    response.resume().on("end", () => server.close());
  });
});
