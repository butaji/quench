const assert = require("assert");
const http = require("http");

const server = http.createServer((req, res) => res.end("ok"));
let timerCalled = false;
server.listen(0, () => {
  setTimeout(() => {
    timerCalled = true;
  }, 100).unref();
  server.close(() => {
    setTimeout(() => assert.strictEqual(timerCalled, false), 1);
  });
});
