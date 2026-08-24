'use strict';
const assert = require('assert');
const { Readable } = require('stream');

const readable = new Readable();
readable._read = () => {};
const state = readable._readableState;
state.reading = true;
assert.strictEqual(readable.unshift(Buffer.alloc(0)), true);
assert.strictEqual(state.reading, true);
assert.strictEqual(readable.unshift(''), true);
assert.strictEqual(state.reading, true);
