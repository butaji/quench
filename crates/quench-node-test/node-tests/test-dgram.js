// Node compat: dgram stub.
const dgram = require('node:dgram');
if (typeof dgram !== 'object') throw new Error('dgram: ' + typeof dgram);
if (typeof dgram.createSocket !== 'function') throw new Error('createSocket: ' + typeof dgram.createSocket);
console.log('dgram: ok');
