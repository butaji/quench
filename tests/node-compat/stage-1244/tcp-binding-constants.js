const assert = require("assert");
const { internalBinding } = require("internal/test/binding");

const { TCP, constants } = internalBinding("tcp_wrap");
const handle = new TCP(constants.SOCKET);
assert.strictEqual(typeof handle.listen, "function");
assert.strictEqual(typeof handle.close, "function");
