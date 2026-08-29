const assert = require("assert");
const { Duplex } = require("stream");

const duplex = Duplex({ readable: false, writable: false });
assert(duplex instanceof Duplex);
assert.strictEqual(duplex.readable, false);
assert.strictEqual(duplex.writable, false);
