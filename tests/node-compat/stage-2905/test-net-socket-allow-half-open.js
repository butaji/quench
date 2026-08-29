'use strict';

const assert = require('node:assert');
const net = require('node:net');

const socket = new net.Socket();
assert.strictEqual(socket.allowHalfOpen, false);
socket.destroy();
