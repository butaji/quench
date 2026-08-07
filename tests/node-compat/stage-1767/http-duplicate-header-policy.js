const assert = require("assert");
const http = require("http");

const server = http.createServer(
  { joinDuplicateHeaders: true },
  (request, response) => {
    assert.strictEqual(request.headers.authorization, "1, 2");
    response.writeHead(200, ["authorization", "3", "authorization", "4"]);
    assert.strictEqual(response.headers.authorization, "3, 4");
    response.end();
  },
);
server.listen(0, () => {
  http.get({
    port: server.address().port,
    headers: ["authorization", "1", "authorization", "2"],
    joinDuplicateHeaders: false,
  }, (response) => {
    response.resume().on("end", () => server.close());
  });
});
