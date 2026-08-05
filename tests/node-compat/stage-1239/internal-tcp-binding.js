const assert = require("assert");
const { internalBinding } = require("internal/test/binding");

const { TCPWrap } = internalBinding("tcp_wrap");
assert.strictEqual(typeof TCPWrap.prototype.setNoDelay, "function");
