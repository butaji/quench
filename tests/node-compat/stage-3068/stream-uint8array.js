'use strict';
const common = require('../../node/test/common');
const assert = require('assert');
const { Readable, Writable } = require('stream');

const ABC = new Uint8Array([0x41, 0x42, 0x43]);
const DEF = new Uint8Array([0x44, 0x45, 0x46]);
const GHI = new Uint8Array([0x47, 0x48, 0x49]);

{
  let n = 0;
  const writable = new Writable({
    write: common.mustCall((chunk, encoding, cb) => {
      assert(chunk instanceof Buffer);
      assert.strictEqual(String(chunk), n++ === 0 ? 'ABC' : 'DEF');
      cb();
    }, 2),
  });
  writable.write(ABC);
  writable.end(DEF);
}

{
  const writable = new Writable({
    objectMode: true,
    write: common.mustCall((chunk, encoding, cb) => {
      assert(!(chunk instanceof Buffer));
      assert(chunk instanceof Uint8Array);
      assert.strictEqual(chunk, ABC);
      assert.strictEqual(encoding, undefined);
      cb();
    }),
  });
  writable.end(ABC);
}

{
  let callback;
  const writable = new Writable({
    write: common.mustCall((chunk, encoding, cb) => {
      assert(chunk instanceof Buffer);
      assert.strictEqual(encoding, 'buffer');
      assert.strictEqual(String(chunk), 'ABC');
      callback = cb;
    }),
    writev: common.mustCall((chunks, cb) => {
      assert.strictEqual(chunks.length, 2);
      assert.strictEqual(chunks[0].encoding, 'buffer');
      assert.strictEqual(chunks[1].encoding, 'buffer');
      assert.strictEqual(chunks[0].chunk + chunks[1].chunk, 'DEFGHI');
      cb();
    }),
  });
  writable.write(ABC);
  writable.write(DEF);
  writable.end(GHI);
  callback();
}

{
  const readable = new Readable({ read() {} });
  readable.push(DEF);
  readable.unshift(ABC);
  const first = readable.read();
  assert(first instanceof Buffer);
  assert.deepStrictEqual([...first], [...ABC]);
  const second = readable.read();
  assert(second instanceof Buffer);
  assert.deepStrictEqual([...second], [...DEF]);
}

{
  const readable = new Readable({ read() {} });
  readable.setEncoding('utf8');
  readable.push(DEF);
  readable.unshift(ABC);
  assert.strictEqual(readable.read(), 'ABCDEF');
}
