'use strict';

const assert = require('assert');
const net = require('net');

for (const value of [true, [], {}, null]) {
  for (const connect of [
    () => net.connect({ port: value }),
    () => net.createConnection({ port: value }),
    () => new net.Socket().connect({ port: value }),
  ]) {
    assert.throws(connect, { code: 'ERR_INVALID_ARG_TYPE', name: 'TypeError' });
  }
}
