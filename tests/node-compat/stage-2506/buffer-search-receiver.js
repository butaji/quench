const assert = require('node:assert');
const buffer = require('buffer');

assert.throws(() => {
  new buffer.Buffer.prototype.lastIndexOf(1, 'str');
}, {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
  message: 'The "buffer" argument must be an instance of Buffer, ' +
    'TypedArray, or DataView. ' +
    'Received an instance of lastIndexOf'
});
