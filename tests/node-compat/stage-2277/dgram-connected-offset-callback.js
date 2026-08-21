const assert = require("assert");
const dgram = require("dgram");

const socket = dgram.createSocket("udp4");
socket.connectSync(12345);
assert.doesNotThrow(() => socket.send(Buffer.from("abc"), 0, 3, () => {}));
socket.close();
console.log("dgram connected offset callback passed");
