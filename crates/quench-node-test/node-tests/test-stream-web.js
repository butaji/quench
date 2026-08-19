// Node compat: stream/web + stream/consumers shape.
const web = require('node:stream/web');
const cons = require('node:stream/consumers');
if (typeof web.ReadableStream !== 'function') throw new Error('ReadableStream');
if (typeof cons.text !== 'function') throw new Error('text');
console.log('stream/web+consumers: ok');
