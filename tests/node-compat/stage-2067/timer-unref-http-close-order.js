const assert = require("assert");
const http = require("http");

const server = http.createServer((req, res) => res.end("ok"));
let fired = false;
server.listen(0, () => {
  http.get({ port: server.address().port }, (res) => {
    res.resume();
    setTimeout(() => {
      fired = true;
    }, 100).unref();
    server.close(() => {
      setTimeout(() => assert.strictEqual(fired, false), 1);
    });
  });
});
