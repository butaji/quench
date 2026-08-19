// Node compat: stream/promises stub.
const sp = require('node:stream/promises');
if (typeof sp !== 'object') throw new Error('stream/promises: ' + typeof sp);
if (typeof sp.finished !== 'function') throw new Error('finished: ' + typeof sp.finished);
console.log('stream/promises: ok');
