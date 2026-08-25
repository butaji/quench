'use strict';

const net = require('net');

const invalidPort = -1 >>> 0;

for (const listen of [
  () => net.Server().listen({ port: invalidPort }),
  () => net.Server().listen(invalidPort),
  () => net.Server().listen(invalidPort, '0.0.0.0'),
]) {
  try {
    listen();
    throw new Error('listen accepted an invalid port');
  } catch (error) {
    if (error.code !== 'ERR_SOCKET_BAD_PORT' || error.name !== 'RangeError') {
      throw error;
    }
  }
}
