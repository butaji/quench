'use strict';

const assert = require('node:assert');
const { spawn } = require('node:child_process');

assert.throws(
  () => spawn(process.argv[0], [], { cwd: new URL('http://example.com/') }),
  /The URL must be of scheme file/,
);
assert.throws(
  () => spawn(process.argv[0], [], { cwd: new URL('file://host/dev/null') }),
  /File URL host must be "localhost" or empty on/,
);
