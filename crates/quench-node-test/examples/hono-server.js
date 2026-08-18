// Smallest possible Hono-style server using only the Node API
// surface that `quench-node` exposes. The host implements each
// `node:` builtin in pure Rust; this script is plain JavaScript
// that uses only what the host provides.

const { createServer } = require('node:http');
const { EventEmitter } = require('node:events');
const { Buffer } = require('node:buffer');

const ee = new EventEmitter();
const bus = new EventEmitter();

ee.on('ping', () => bus.emit('pong'));
ee.emit('ping');
bus.emit('pong');

console.log('hono-server: started');
console.log('Buffer: %s', typeof Buffer);
console.log('Buffer.from: %s', typeof Buffer.from);
console.log('util.format: %s', require('node:util').format('hi %s', 'there'));
console.log('path.join: %s', require('node:path').join('/tmp', 'a', 'b.js'));
console.log('url.query: %s', require('node:url').parse('http://x/y?z=1').query);
console.log('querystring: %s', require('node:querystring').parse('a=1&b=2').a[0]);
console.log('os.platform: %s', require('node:os').platform);
console.log('process.cwd: %s', process.cwd());
console.log('process.version: %s', process.version);
console.log('setTimeout id: %s', setTimeout(() => {}, 0));
console.log('net.isIP: %s', require('node:net').isIP('127.0.0.1'));
console.log('net.isIPv4: %s', require('node:net').isIPv4('::1'));
console.log('hono-server: ok');
