// Node compat: util module.
const util = require('node:util');
if (typeof util.format !== 'function') throw new Error('format: ' + typeof util.format);
if (typeof util.inspect !== 'function') throw new Error('inspect: ' + typeof util.inspect);
const s = util.format('hi %s, %d items', 'there', 42);
if (!(s === 'hi there, 42 items')) throw new Error('format=' + s);
console.log('util: %s', s);
