const assert = require("assert");
const { _createSocketHandle } = require("internal/dgram");
const { internalBinding } = require("internal/test/binding");
const { UDP } = internalBinding("udp_wrap");

const invalid = _createSocketHandle("127.0.0.1", 0, "udp4", 42);
assert(invalid < 0);
const raw = new UDP();
assert.strictEqual(raw.bind("127.0.0.1", 0, 0), 0);
const adopted = _createSocketHandle(null, 0, "udp4", raw.fd);
assert(adopted instanceof UDP);
assert(adopted.fd > 0);
console.log("dgram handle fd passed");
