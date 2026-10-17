'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { vfsState } = require('internal/fs/utils');
const saved = vfsState.handlers;
try {
  for (const name of ['writeFileSync', 'appendFileSync']) {
    const path = {}, data = {};
    let log = [];
    const options = {
      get encoding() { log.push('encoding'); return 'utf8'; },
      get flush() { log.push('flush'); return false; },
      get mode() { log.push('mode'); return '600'; },
    };
    const handlers = { [name](...args) {
      log.push('handler');
      assert.equal(this, handlers);
      assert.equal(args.length, 3);
      assert.equal(args[0], path);
      assert.equal(args[1], data);
      assert.equal(args[2], options);
      return false;
    } };
    vfsState.handlers = handlers;
    assert.equal(fs[name](path, data, options, 'drop-extra'), undefined);
    assert.deepEqual(log, name === 'writeFileSync' ? ['encoding', 'encoding', 'flush', 'mode', 'handler'] : ['encoding', 'encoding', 'mode', 'handler']);
    vfsState.handlers = { [name](p, d, actual) {
      assert.equal(p, path); assert.equal(d, data);
      assert.deepEqual(actual, name === 'writeFileSync' ? { encoding: 'utf8', mode: 0o666, flag: 'w', flush: false } : { encoding: 'utf8', mode: 0o666, flag: 'a' });
      return 0;
    } };
    assert.equal(fs[name](path, data), undefined);
    let called = false;
    vfsState.handlers = { [name]() { called = true; return true; } };
    assert.throws(() => fs[name](path, data, { mode: {} }), { code: 'ERR_INVALID_ARG_TYPE' });
    assert.equal(called, false);
    if (name === 'writeFileSync') {
      log = [];
      assert.throws(() => fs[name](path, data, {
        encoding: 'utf8', get flush() { log.push('flush'); return 'bad'; },
        get mode() { log.push('mode'); return 0o600; },
      }), { code: 'ERR_INVALID_ARG_TYPE' });
      assert.deepEqual(log, ['flush']);
    }
    const thrown = {};
    vfsState.handlers = { [name]() { throw thrown; } };
    assert.throws(() => fs[name](path, data), error => error === thrown);
  }
} finally { vfsState.handlers = saved; }
console.log('SDK write options identity/defaults/getter order/validation/void projection passed');
