const assert = require("assert");
const http = require("http");

const server = http.createServer((request, response) => {
  assert(response.socket);
  response.cork();
  response.cork();
  assert.strictEqual(response.writableCorked, 2);
  assert.strictEqual(response.socket.writableCorked, 2);
  response.uncork();
  assert.strictEqual(response.writableCorked, 1);
  response.end();
});
server.listen(0, () => {
  http.get({ port: server.address().port }, (response) => {
    response.resume().on("end", () => server.close());
  });
});
