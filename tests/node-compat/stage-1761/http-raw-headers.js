const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert(Array.isArray(request.rawHeaders));
  assert(request.rawHeaders.some((name) => name.toLowerCase() === "host"));
  assert.strictEqual(request.method, "DELETE");
  request.on("data", () => assert.fail("unexpected DELETE body"));
  response.end("ok");
});

server.listen(0, () => {
  http.request(
    { port: server.address().port, method: "DELETE" },
    (response) => {
      response.resume().on("end", () => server.close());
    },
  ).end();
});
