'use strict';

const assert = require('assert');
const dns = require('dns');
const net = require('net');

const hints = (dns.ADDRCONFIG | dns.V4MAPPED | dns.ALL) + 42;
assert.throws(() => net.connect({ port: 42, hints }), {
  code: 'ERR_INVALID_ARG_VALUE',
  name: 'TypeError',
});
