const assert = require("assert");
const net = require("net");

const socket = new net.Socket();
let closes = 0;
socket.on("close", () => closes++);
socket.destroy();
socket.destroy();
setTimeout(() => assert.strictEqual(closes, 1), 0);
