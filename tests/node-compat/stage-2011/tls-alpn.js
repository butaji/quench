const assert = require("assert");
const tls = require("tls");
const out = {};
tls.convertALPNProtocols(Buffer.from("abcd"), out);
assert.strictEqual(typeof out.ALPNProtocols.write, "function");
out.ALPNProtocols.write("efgh");
assert.strictEqual(out.ALPNProtocols.toString(), "efgh");
