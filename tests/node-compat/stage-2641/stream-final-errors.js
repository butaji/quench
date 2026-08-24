'use strict';
const common = require('../../node/test/common');
const { Writable } = require('stream');
{
  const w = new Writable();
  w._write = (chunk, encoding, cb) => process.nextTick(cb);
  w.on('error', common.mustCall());
  w.on('finish', common.mustNotCall());
  w.on('prefinish', () => w.write("shouldn't write in prefinish listener"));
  w.end();
}
{
  const w = new Writable();
  w._write = (chunk, encoding, cb) => process.nextTick(cb);
  w.on('error', common.mustCall());
  w.on('finish', () => w.write("shouldn't write in finish listener"));
  w.end();
}

function source() {
  const readable = new (require('stream').Readable)();
  readable.push('ok');
  readable.push(null);
  readable._read = () => {};
  return readable;
}

{
  const writable = new Writable();
  writable._write = (chunk, encoding, done) => setImmediate(done, new Error('pipe'));
  writable.on('finish', common.mustNotCall());
  writable.on('error', common.mustCall());
  source().pipe(writable);
}

{
  const writable = new Writable();
  writable._write = (chunk, encoding, done) => done(new Error('pipe'));
  writable.on('finish', common.mustNotCall());
  writable.on('error', common.mustCall());
  source().pipe(writable);
}
