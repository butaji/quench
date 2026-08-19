// Node compat: Buffer round-trip.
const { Buffer } = require('node:buffer');
const a = Buffer.from([1, 2, 3]);
if (!(a.length === 3)) throw new Error('len=' + a.length);
if (!(a[0] === 1)) throw new Error('a0=' + a[0]);
if (!(a[1] === 2)) throw new Error('a1=' + a[1]);
if (!(a[2] === 3)) throw new Error('a2=' + a[2]);
const b = Buffer.alloc(4);
if (!(b.length === 4)) throw new Error('alloc-len=' + b.length);
const c = Buffer.from('hi');
if (!(c.length === 2)) throw new Error('string-len=' + c.length);
console.log('buffer: ' + a.length + ' ' + b.length + ' ' + c.length);
