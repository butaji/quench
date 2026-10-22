'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { vfsState } = require('internal/fs/utils');
const savedState = vfsState.handlers;
const saved = Object.fromEntries(['writeFileSync', 'openSync', 'writeSync', 'closeSync'].map(name => [name, fs[name]]));
const cases = [-1, -0, 2 ** 31, 4294967295];
try {
  vfsState.handlers = null;
  for (const value of cases) {
    const owned = value === -1 || Object.is(value, -0);
    let calls = [];
    const openError = {};
    fs.openSync = function(path, flag, mode) {
      assert.ok(Object.is(path, value)); calls.push('open'); throw openError;
    };
    fs.writeSync = function(fd, bytes, offset, length) {
      assert.ok(Object.is(fd, value)); calls.push('write'); return length;
    };
    fs.closeSync = function() { calls.push('close'); };
    if (owned) {
      assert.equal(saved.writeFileSync(value, Buffer.from('x')), undefined);
      assert.deepEqual(calls, ['write']);
    } else {
      assert.throws(() => saved.writeFileSync(value, Buffer.from('x')), error => error === openError);
      assert.deepEqual(calls, ['open']);
    }
    calls = [];
    let outcome = 'ok';
    try { saved.writeFileSync(value, Object.is(value, -0) ? '' : 'x'); }
    catch (error) {
      outcome = error.code;
      assert.equal(error.code, owned ? 'EBADF' : 'ERR_INVALID_ARG_TYPE');
      if (owned) assert.equal(error.syscall, 'write');
    }
    if (!Object.is(value, -0)) assert.notEqual(outcome, 'ok');
    // fd0's native writability is host policy; negative zero still reaches
    // the native UTF8 continuation rather than the public open/write methods.
    assert.deepEqual(calls, []);

    const marker = {};
    fs.writeFileSync = function(path, data, options) {
      assert.ok(Object.is(path, value));
      assert.equal(options.flag, owned ? 'a' : 'w');
      return marker;
    };
    assert.equal(fs.appendFileSync(value, Buffer.from('x'), { flag: 'w' }), undefined);
    fs.writeFileSync = () => { throw marker; };
    assert.throws(() => fs.appendFileSync(value, 'x'), error => error === marker);
    fs.writeFileSync = saved.writeFileSync;
  }
} finally { vfsState.handlers = savedState; Object.assign(fs, saved); }
console.log('SDK fd domains and append delegated void/throw identity passed');
