const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert.strictEqual(request.url, "/testpath");
  response.end("ok");
});
server.listen(0, () => {
  http.get(
    "http://example.com/ignored",
    { hostname: "localhost", port: server.address().port, path: "/testpath" },
    (response) => response.resume().on("end", () => server.close()),
  );
});
