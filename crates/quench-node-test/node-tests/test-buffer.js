// Node compat: Buffer.concat + from(string) + from(array).
const { Buffer } = require('node:buffer');
const a = Buffer.from('hello ');
const b = Buffer.from('world');
const c = Buffer.concat([a, b]);
if (!(c.length === 11)) throw new Error('concat-len=' + c.length);
if (!(c[0] === 'h'.charCodeAt(0))) throw new Error('concat-0=' + c[0]);
if (!(c[6] === 'w'.charCodeAt(0))) throw new Error('concat-6=' + c[6]);
const d = Buffer.from([1, 2, 3, 4]);
if (!(d.length === 4)) throw new Error('from-array-len=' + d.length);
if (!(d[0] === 1)) throw new Error('from-array-0=' + d[0]);
if (!(d[3] === 4)) throw new Error('from-array-3=' + d[3]);
const e = Buffer.from('hi');
if (!(e.length === 2)) throw new Error('from-string=' + e.length);
const f = Buffer.alloc(3);
if (!(f.length === 3)) throw new Error('alloc=' + f.length);
console.log('buffer: %s', c.length);
