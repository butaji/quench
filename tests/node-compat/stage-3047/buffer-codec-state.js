'use strict';

// Buffer's string codecs share one UTF-16 -> UTF-8 fact path.  Exercise the
// observable boundaries without relying on a particular VM string layout.
const assert = require('assert');

const loneHigh = '\ud83d';
const loneLow = '\ude00';
const cases = [
  ['bmp', '日本語'],
  ['astral', '😀💩'],
  ['lone-high', `a${loneHigh}b`],
  ['lone-low', `a${loneLow}b`],
  ['pair-and-lone', `${loneHigh}\ude00${loneHigh}`],
];

for (const [label, input] of cases) {
  const expected = Buffer.from(new TextEncoder().encode(input));
  assert.deepStrictEqual(Buffer.from(input, 'utf8'), expected, `${label}: from`);
  assert.strictEqual(Buffer.byteLength(input, 'utf8'), expected.length, `${label}: length`);

  const exact = Buffer.alloc(expected.length);
  assert.strictEqual(exact.write(input, 0, expected.length, 'utf8'), expected.length);
  assert.deepStrictEqual(exact, expected, `${label}: exact write`);

  for (let size = 0; size <= expected.length; size++) {
    const target = Buffer.alloc(size + 1, 0xaa);
    const written = target.write(input, 0, size, 'utf8');
    assert.ok(written <= size, `${label}: bounds`);
    assert.deepStrictEqual(target.subarray(0, written), expected.subarray(0, written));
    assert.strictEqual(target[size], 0xaa, `${label}: no overrun`);
    if (written < expected.length) {
      assert.notStrictEqual(expected[written] & 0xc0, 0x80, `${label}: split sequence`);
    }
  }
}

const repeated = '😀日本語'.repeat(2_000);
const encoded = Buffer.from(repeated);
assert.deepStrictEqual(encoded, Buffer.from(new TextEncoder().encode(repeated)));
