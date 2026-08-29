'use strict';

const assert = require('assert');
const net = require('net');

assert.throws(() => net.connect({ port: null }), {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
});
