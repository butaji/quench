"use strict";
const assert = require("assert");
const net = require("net");
const socket = net.Stream();
assert.strictEqual(socket.pending, true);
assert.strictEqual(socket.connecting, false);
assert.strictEqual(socket.readyState, "closed");
