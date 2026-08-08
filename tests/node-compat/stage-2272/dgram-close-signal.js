const assert = require("assert");
const dgram = require("dgram");

assert.throws(() => dgram.createSocket({ type: "udp4", signal: {} }), {
  code: "ERR_INVALID_ARG_TYPE"
});

const controller = new AbortController();
const socket = dgram.createSocket({ type: "udp4", signal: controller.signal });
socket.on("close", () => console.log("dgram close signal passed"));
controller.abort();
