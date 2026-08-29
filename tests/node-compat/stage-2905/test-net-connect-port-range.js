'use strict';

const assert = require('assert');
const net = require('net');

for (const value of ['', ' ', '0x', '-0x1', NaN, Infinity, -1, 65536]) {
  for (const connect of [
    () => net.connect({ port: value, family: 4 }),
    () => net.createConnection({ port: value, family: 4 }),
    () => new net.Socket().connect({ port: value, family: 4 }),
  ]) {
    assert.throws(connect, { code: 'ERR_SOCKET_BAD_PORT', name: 'RangeError' }, String(value));
  }
}
