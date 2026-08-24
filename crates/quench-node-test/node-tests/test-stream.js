// Node compat: stream module shape.
const stream = require('node:stream');
if (typeof stream.Readable !== 'function') throw new Error('Readable: ' + typeof stream.Readable);
if (typeof stream.Writable !== 'function') throw new Error('Writable: ' + typeof stream.Writable);
if (typeof stream.Duplex !== 'function') throw new Error('Duplex: ' + typeof stream.Duplex);
if (typeof stream.Transform !== 'function') throw new Error('Transform: ' + typeof stream.Transform);
if (typeof stream.pipeline !== 'function') throw new Error('pipeline: ' + typeof stream.pipeline);
const readable = stream.Readable({ read() {} });
if (!(readable instanceof stream.Readable)) throw new Error('Readable factory identity');
readable.destroy();
console.log('stream: ok');
