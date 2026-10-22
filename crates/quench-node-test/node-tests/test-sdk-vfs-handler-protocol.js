'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { vfsState } = require('internal/fs/utils');
const saved = vfsState.handlers;
const source = fs.readFileSync(__filename, 'utf8');
try {
  for (const [name, count] of [['existsSync', 1], ['readFileSync', 2]]) {
    for (const inherited of [false, true]) {
      for (const value of [false, 0, null, '', NaN]) {
        const owner = {};
        const handlers = inherited ? Object.create(owner) : owner;
        Object.defineProperty(owner, name, { configurable: true, get() {
          assert.equal(this, handlers);
          return function() {
            assert.equal(this, handlers);
            assert.equal(arguments.length, count);
            assert.equal(arguments[0], 'guest-path');
            if (count === 2) assert.equal(arguments[1], undefined);
            return value;
          };
        } });
        vfsState.handlers = handlers;
        assert.ok(Object.is(fs[name]('guest-path'), value));
        assert.ok(Object.is(fs[name]('guest-path', undefined, 'drop-extra'), value));
      }
    }
    vfsState.handlers = { [name]() {
      assert.equal(this, vfsState.handlers);
      assert.equal(arguments.length, count);
      assert.equal(arguments[0], undefined);
      if (count === 2) assert.equal(arguments[1], undefined);
      return 'omitted';
    } };
    assert.equal(fs[name](), 'omitted');
    const thrown = {};
    vfsState.handlers = { [name]() { throw thrown; } };
    assert.throws(() => fs[name](), error => error === thrown);
    for (const handlers of [{}, { [name]: 0 }, undefined]) {
      vfsState.handlers = handlers;
      assert.throws(() => fs[name](__filename), TypeError);
    }
    vfsState.handlers = { [name]() { return undefined; } };
    assert.equal(fs[name](__filename, 'utf8'), name === 'existsSync' ? true : source);
    vfsState.handlers = null;
    assert.equal(fs[name](__filename, 'utf8'), name === 'existsSync' ? true : source);
  }
} finally {
  vfsState.handlers = saved;
}
console.log('SDK handler receiver, argc, lookup, values, errors and fallback passed');
