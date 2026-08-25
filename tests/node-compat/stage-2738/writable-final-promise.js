const assert = require('assert');
const { Duplex } = require('stream');
const { setTimeout } = require('timers/promises');
class Foo extends Duplex {
  async _final(callback) {
    await setTimeout(1);
    callback();
  }
  _read() {}
}
let writes = 0;
let finished = false;
let streamError;
let ended = false;
const foo = new Foo();
foo._write = (_chunk, _encoding, callback) => { writes++; callback(); };
foo.on('finish', () => { finished = true; });
foo.on('error', (error) => { streamError = error; });
foo.end('test', () => { ended = true; });
setTimeout(20).then(() => {
  assert.strictEqual(writes, 1);
  assert.strictEqual(finished, true);
  assert.strictEqual(ended, true);
  assert.strictEqual(streamError, undefined);
});
