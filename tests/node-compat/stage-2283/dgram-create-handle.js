const assert = require("assert");
const { _createSocketHandle } = require("internal/dgram");
const { internalBinding } = require("internal/test/binding");
const { UDP } = internalBinding("udp_wrap");

const unbound = _createSocketHandle(null, null, "udp4");
assert(unbound instanceof UDP);
assert.strictEqual(typeof unbound.fd, "number");
assert(unbound.fd < 0);
const bound = _createSocketHandle("127.0.0.1", 0, "udp4");
assert(bound instanceof UDP);
assert(bound.fd > 0);
console.log("dgram create handle passed");
