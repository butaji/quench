"use strict";
const assert = require("assert");
const net = require("net");
const server = net.Server();
assert.strictEqual(typeof server.listen, "function");
assert.strictEqual(server.listening, false);
server.listen(0, () => server.close());
