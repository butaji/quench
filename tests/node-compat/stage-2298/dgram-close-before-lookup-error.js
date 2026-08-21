const assert = require("assert");
const dgram = require("dgram");

let lookupCallback;
const socket = dgram.createSocket({
  type: "udp4",
  lookup(_host, _family, callback) {
    lookupCallback = callback;
  },
});
let errors = 0;
socket.on("error", () => errors++);
socket.bind(12345, "localhost");
socket.close();
setImmediate(() => {
  lookupCallback(new Error("late lookup failure"));
  setImmediate(() => {
    assert.strictEqual(errors, 0);
    console.log("dgram close-before-lookup error passed");
  });
});
