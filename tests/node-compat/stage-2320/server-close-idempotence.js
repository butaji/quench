const assert = require("assert");
const net = require("net");

const server = net.createServer();
let callbacks = 0;
server.listen(0, () => {
  server.close(() => {
    callbacks++;
  });
  queueMicrotask(() => assert.strictEqual(callbacks, 1));
});
