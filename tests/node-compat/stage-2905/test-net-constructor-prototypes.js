'use strict';

const assert = require('assert');
const net = require('net');

const socket = new net.Socket();
assert.strictEqual(socket instanceof net.Socket, true);
assert.strictEqual(typeof net.Socket.prototype, 'object');
