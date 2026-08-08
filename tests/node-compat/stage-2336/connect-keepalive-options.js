const assert = require("assert");
const net = require("net");

const calls = [];
const socket = new net.Socket({
  handle: { setKeepAlive: (enabled, delay) => calls.push([enabled, delay]) }
});
socket.connect({ port: 1, keepAlive: true, keepAliveInitialDelay: 3000 });
assert.deepStrictEqual(calls, [[true, 3]]);
socket.destroy();
console.log("connect keepalive options passed");
