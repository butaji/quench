// Node compat: tls + cluster shape.
const tls = require('node:tls');
const cluster = require('node:cluster');
if (typeof tls !== 'object') throw new Error('tls: ' + typeof tls);
if (typeof cluster !== 'object') throw new Error('cluster: ' + typeof cluster);
console.log('tls+cluster: ok');
