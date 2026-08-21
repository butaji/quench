const assert = require("assert");
const { Duplex } = require("stream");

const duplex = Duplex();
assert.ok(duplex instanceof Duplex);
assert.ok(duplex._readableState);
assert.ok(duplex._writableState);

console.log("stream duplex callable pass");
