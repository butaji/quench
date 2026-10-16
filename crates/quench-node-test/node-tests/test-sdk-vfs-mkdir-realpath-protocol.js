'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { vfsState } = require('internal/fs/utils');
const saved = vfsState.handlers;
try {
  for (const name of ['mkdirSync', 'realpathSync']) {
    const path = {};
    for (const publicResult of [false, 0, null, '', NaN, undefined]) {
      const owner = {};
      const handlers = Object.create(owner);
      owner[name] = function() {
        assert.equal(this, handlers);
        assert.equal(arguments.length, 2);
        assert.equal(arguments[0], path);
        assert.equal(arguments[1], undefined);
        return name === 'mkdirSync' ? { result: publicResult } : publicResult;
      };
      vfsState.handlers = handlers;
      if (name === 'realpathSync' && publicResult === undefined) {
        // Undefined selects the existing host fallback, tested with a valid path below.
        continue;
      } else {
        assert.ok(Object.is(fs[name](path), publicResult));
        assert.ok(Object.is(fs[name](path, undefined, 'drop-extra'), publicResult));
      }
    }
    const marker = {};
    vfsState.handlers = { [name]() { throw marker; } };
    assert.throws(() => fs[name](), error => error === marker);
    for (const handlers of [{}, { [name]: false }, undefined]) {
      vfsState.handlers = handlers;
      assert.throws(() => fs[name](__filename), TypeError);
    }
    vfsState.handlers = { [name]() { return undefined; } };
    if (name === 'mkdirSync') assert.throws(() => fs[name](__dirname), { code: 'EEXIST' });
    else assert.equal(fs[name](__filename), __filename);
  }
  vfsState.handlers = { mkdirSync() { return null; } };
  assert.throws(() => fs.mkdirSync(), TypeError);
  vfsState.handlers = { mkdirSync() { return Object.create({ result: 'inherited' }); } };
  assert.equal(fs.mkdirSync(), 'inherited');
} finally { vfsState.handlers = saved; }
console.log('SDK mkdir/realpath fixed argc, receiver, before-validation dispatch and result protocol passed');
