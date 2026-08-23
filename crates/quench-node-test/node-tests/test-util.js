// Node compat: util module.
const util = require('node:util');
if (typeof util.format !== 'function') throw new Error('format: ' + typeof util.format);
if (typeof util.inspect !== 'function') throw new Error('inspect: ' + typeof util.inspect);
const s = util.format('hi %s, %d items', 'there', 42);
if (!(s === 'hi there, 42 items')) throw new Error('format=' + s);
console.log('util: %s', s);
const p = util.promisify((x, cb) => cb(null, x + 1));
p(4).then(v => { if (v !== 5) throw new Error('promisify=' + v); });
if (!util.types.isArrayBuffer(new ArrayBuffer(1))) throw new Error('ArrayBuffer');
if (!util.types.isDate(new Date())) throw new Error('Date');
util.callbackify((x) => Promise.resolve(x))(7, (err, v) => { if (err || v !== 7) throw new Error('callbackify'); });
