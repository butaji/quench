'use strict';
const assert = require('assert');
const { Writable } = require('stream');

const seen = [];
const writable = new Writable({
  write(chunk, encoding, callback) {
    seen.push([chunk.toString(), encoding]);
    callback();
  }
});
writable.cork();
writable.write('a');
assert.deepStrictEqual(seen, []);
assert.strictEqual(writable.writableCorked, 1);
writable.uncork();
assert.deepStrictEqual(seen, [['a', undefined]]);
assert.strictEqual(writable.writableCorked, 0);
writable.end();
