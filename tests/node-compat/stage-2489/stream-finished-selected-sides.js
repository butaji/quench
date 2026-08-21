const assert = require("assert");
const { PassThrough, finished } = require("stream");

const both = new PassThrough();
let bothCalls = 0;
finished(both, (error) => {
  assert.ifError(error);
  bothCalls++;
});
both.end("buffered");

const writableOnly = new PassThrough();
let writableCalls = 0;
finished(writableOnly, { readable: false }, (error) => {
  assert.ifError(error);
  writableCalls++;
});
writableOnly.end("buffered");

setImmediate(() => {
  assert.strictEqual(bothCalls, 0);
  assert.strictEqual(writableCalls, 1);
  both.resume();
  setImmediate(() => assert.strictEqual(bothCalls, 1));
});
