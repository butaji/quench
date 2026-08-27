'use strict';

const assert = require('assert');
const net = require('net');

for (const lookup of ['foobar', 1, {}, []]) {
  assert.throws(() => net.connect({ host: 'localhost', port: 0, lookup }), {
    code: 'ERR_INVALID_ARG_TYPE',
    name: 'TypeError',
  });
}
