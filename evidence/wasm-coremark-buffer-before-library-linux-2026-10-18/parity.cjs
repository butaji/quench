// Usage: node parity.cjs <case>. Each case reads files before touching Buffer, then observes.
const fs = require('fs');
const path = require('path');
const dir = __dirname;
const out = [];
const log = (...a) => out.push(a.map(v => typeof v === 'string' ? v : JSON.stringify(v)).join(' '));
const desc = (o, k) => { const d = Object.getOwnPropertyDescriptor(o, k); return d && ['value' in d ? 'data' : 'acc', d.enumerable, d.configurable, d.writable].join(','); };
const c = process.argv[2];
// Node's library loads internal/buffer.js without buffer.js for streams and encodings.
if (c === 'stream-first') require('stream');
const a = fs.readFileSync(path.join(dir, 'small.bin'));
const b = fs.readFileSync(path.join(dir, 'big.bin'));
const t = fs.readFileSync(path.join(dir, 'tiny.bin'));
const p = Object.getPrototypeOf(a);
if (c === 'keys') {
  log('ownKeys', Reflect.ownKeys(p).map(String));
  log('keys', Object.keys(p));
} else if (c === 'proto') {
  log(p === Object.getPrototypeOf(b), p === Object.getPrototypeOf(t), Object.getPrototypeOf(p) === Uint8Array.prototype);
  log(p === Buffer.prototype, a instanceof Buffer, a.constructor === Buffer);
} else if (c === 'ctor-first') {
  log(a.constructor === Buffer, a.constructor.name, p.constructor.name);
} else if (c === 'pool') {
  log(a.byteLength, a.buffer.byteLength, a.byteOffset % 8, b.buffer.byteLength, b.byteOffset, t.buffer.byteLength);
  log(a.buffer === t.buffer, (t.byteOffset - a.byteOffset));
  const x = Buffer.allocUnsafe(10);
  log(x.buffer === a.buffer, x.byteOffset - t.byteOffset);
  try { a.buffer.transfer(); log('transferred'); } catch (e) { log('transfer', e.name); }
} else if (c === 'inspect') {
  log(require('util').inspect(t).slice(0, 12));
  log(t.toString('hex').length, desc(p, 'parent'), desc(p, 'toString'), desc(p, 'constructor'));
} else if (c === 'desc-first') {
  log(desc(p, 'parent'), desc(p, 'offset'), desc(p, 'readUInt8'), desc(p, Symbol.for('nodejs.util.inspect.custom')));
  log(Reflect.ownKeys(p).length);
} else if (c === 'write-first') {
  p.toString = function () { return 'mine'; };
  log(a.toString(), Buffer.from('x').toString(), Object.keys(p).indexOf('toString'));
} else if (c === 'delete-first') {
  delete p.toString;
  log(typeof a.toString, Object.keys(p).includes('toString'), String(Object.getOwnPropertyNames(p).length));
} else if (c === 'define-first') {
  Object.defineProperty(p, 'swap16', { value: 1, writable: false, enumerable: false, configurable: true });
  log(desc(p, 'swap16'), Buffer.prototype.swap16, Object.keys(p).length);
} else if (c === 'buffer-first') {
  log(typeof Buffer.alloc, Buffer.prototype === p, Reflect.ownKeys(p).length, a.readUInt8(0) === a[0]);
} else if (c === 'parent') {
  log(a.parent === a.buffer, b.offset === b.byteOffset);
} else if (c === 'freeze') {
  Object.freeze(p);
  log(Object.isFrozen(p), typeof p.toString, Reflect.ownKeys(p).length);
} else if (c === 'forin') {
  const ks = []; for (const k in t) if (!/^\d+$/.test(k)) ks.push(k);
  log(ks.length, ks.slice(0, 3), ks.slice(-3));
} else if (c === 'stream-first') {
  log(typeof a.readUInt8, a.toString('hex', 0, 4), p === Buffer.prototype, Reflect.ownKeys(p).length);
} else if (c === 'sym') {
  const s = Symbol.for('nodejs.util.inspect.custom');
  log(typeof t[s], s in p, Object.getOwnPropertySymbols(p).map(String));
}
console.log(out.join('\n'));
