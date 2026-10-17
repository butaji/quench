'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { vfsState } = require('internal/fs/utils');
const saved = vfsState.handlers;
try {
  const path = {};
  for (const result of [false, 0, null, '', NaN]) {
    const handlers = { openSync(...args) {
      assert.equal(this, handlers); assert.equal(args.length, 3);
      assert.equal(args[0], path); assert.equal(args[1], fs.constants.O_RDONLY);
      assert.equal(args[2], 0o666); return result;
    } };
    vfsState.handlers = handlers;
    assert.ok(Object.is(fs.openSync(path, undefined, undefined, 'drop'), result));
  }
  let called = false;
  vfsState.handlers = { openSync() { called = true; return 1; } };
  assert.throws(() => fs.openSync(path, 'invalid'), { code: 'ERR_INVALID_ARG_VALUE' });
  assert.equal(called, false);
  assert.throws(() => fs.openSync(path, 'r', {}), { code: 'ERR_INVALID_ARG_TYPE' });
  assert.equal(called, false);
  vfsState.handlers = { openSync(...args) { assert.equal(args[1], fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_TRUNC); assert.equal(args[2], 0o600); return 0; } };
  assert.equal(fs.openSync(path, 'w', '600'), 0);
  const marker = {};
  vfsState.handlers = { openSync() { throw marker; } };
  assert.throws(() => fs.openSync(path), error => error === marker);
} finally { vfsState.handlers = saved; }
console.log('SDK open policy-before-handler/raw-path/argc3/receiver/result protocol passed');
