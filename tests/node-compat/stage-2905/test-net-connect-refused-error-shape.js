'use strict';

const assert = require('assert');
const net = require('net');

const socket = net.connect({ port: 0 });
socket.once('error', (error) => {
  assert.match(String(error), /^Error: connect ECONNREFUSED /);
});
