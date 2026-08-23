// Node compat: stream predicate exports (Bun green).
const stream = require('node:stream');
const { Readable, Writable } = require('node:stream');

if (typeof stream.isReadable !== 'function') throw new Error('isReadable missing');
if (typeof stream.isWritable !== 'function') throw new Error('isWritable missing');
if (typeof stream.isErrored !== 'function') throw new Error('isErrored missing');
if (typeof Readable.isDisturbed !== 'function') throw new Error('isDisturbed missing');

// Non-stream values yield a null (falsy) result, matching Node.
if (stream.isReadable(null)) throw new Error('null readable');
if (stream.isReadable({})) throw new Error('empty readable truthy');
if (stream.isWritable(null)) throw new Error('null writable');
if (stream.isWritable({})) throw new Error('empty writable truthy');

// A flowing, non-destroyed readable is readable.
const r = new Readable({ read() {} });
if (stream.isReadable(r) !== true) throw new Error('readable instance');
r.push('x');
if (stream.isReadable(r) !== true) throw new Error('readable buffered');

// A destroyed readable is not readable.
r.destroy();
if (stream.isReadable(r) !== false) throw new Error('destroyed still readable');

// A non-destroyed writable is writable; destroyed is not.
const w = new Writable({ write(c, e, cb) { cb(); } });
if (stream.isWritable(w) !== true) throw new Error('writable instance');
if (stream.isErrored(w) !== false) throw new Error('writable errored');
w.on('error', function () {});
w.destroy(new Error('boom'));
if (stream.isWritable(w) !== false) throw new Error('destroyed writable');
if (stream.isErrored(w) !== true) throw new Error('writable error not surfaced');

console.log('stream-predicates: ok');