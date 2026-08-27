'use strict';

const assert = require('assert');
const net = require('net');

for (const value of [true, false, null]) {
  for (const connect of [
    () => net.connect({ port: value, family: 4 }, () => {}),
    () => net.createConnection({ port: value, family: 4 }, () => {}),
    () => new net.Socket().connect({ port: value, family: 4 }, () => {}),
  ]) {
    assert.throws(connect, { code: 'ERR_INVALID_ARG_TYPE', name: 'TypeError' });
  }
}
