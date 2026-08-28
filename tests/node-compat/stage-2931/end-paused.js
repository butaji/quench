'use strict';
const assert = require('assert');
const { Readable } = require('stream');
const stream = new Readable();
let reads = 0;
stream._read = function () { reads++; this.push(null); };
stream.on('data', () => {});
stream.pause();
setTimeout(() => {
  let ended = false;
  stream.on('end', () => { ended = true; });
  stream.resume();
  setTimeout(() => { assert.strictEqual(reads, 1); assert.strictEqual(ended, true); }, 5);
}, 1);
