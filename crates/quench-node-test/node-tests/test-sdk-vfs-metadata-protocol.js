'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { vfsState } = require('internal/fs/utils');
const stateDescriptor = Object.getOwnPropertyDescriptor(vfsState, 'handlers');
const saved = vfsState.handlers;
try {
  for (const [name, length] of [['statSync', 1], ['lstatSync', 1], ['fstatSync', 1], ['closeSync', 1], ['fchmodSync', 2]]) {
    assert.equal(fs[name].name, name);
    assert.equal(fs[name].length, length);
    assert.equal(Object.hasOwn(fs[name], 'prototype'), true);
    assert.equal(Object.getPrototypeOf(Reflect.construct(Function, [], fs[name])), fs[name].prototype);
  }
  for (const name of ['statSync', 'lstatSync']) {
    const rawPath = {};
    let prior;
    const handlers = { [name](path, options) {
      assert.equal(this, handlers); assert.equal(arguments.length, 2); assert.equal(path, rawPath);
      assert.equal(Object.getPrototypeOf(options), null);
      assert.deepEqual(Object.keys(options), ['bigint', 'throwIfNoEntry']);
      assert.equal(options.bigint, false); assert.equal(options.throwIfNoEntry, true);
      assert.notEqual(options, prior); prior = options;
      return 0;
    } };
    vfsState.handlers = handlers;
    assert.equal(fs[name](rawPath), 0);
    assert.equal(fs[name](rawPath, undefined, 'drop'), 0);
    for (const value of [false, null, '', NaN]) {
      const options = null;
      const owner = { [name](path, actual) { assert.equal(this, inherited); assert.equal(actual, options); return value; } };
      const inherited = Object.create(owner);
      vfsState.handlers = inherited;
      assert.ok(Object.is(fs[name](rawPath, options), value));
    }
    const events = [];
    const marker = { get code() { events.push('code'); return 'ENOENT'; } };
    const options = { get throwIfNoEntry() { events.push('option'); return false; } };
    vfsState.handlers = { [name]() { events.push('call'); throw marker; } };
    assert.equal(fs[name](rawPath, options), undefined);
    assert.deepEqual(events, ['call', 'code', 'option']);
    assert.throws(() => fs[name](rawPath, null), error => error === marker);
    const getterError = {};
    vfsState.handlers = { [name]() { throw { get code() { throw getterError; } }; } };
    assert.throws(() => fs[name](rawPath, options), error => error === getterError);
    Object.defineProperty(vfsState, 'handlers', { configurable: true, get() { throw marker; } });
    assert.throws(() => fs[name](rawPath, { throwIfNoEntry: false }), error => error === marker);
    Object.defineProperty(vfsState, 'handlers', stateDescriptor);
  }
  const rawFd = {};
  const optionError = {};
  for (const name of ['fstatSync', 'closeSync']) {
    const handlers = { [name](fd) { assert.equal(this, handlers); assert.equal(fd, rawFd); assert.equal(arguments.length, 1); return false; } };
    vfsState.handlers = handlers;
    const value = fs[name](rawFd, { get bigint() { throw optionError; } }, 'drop');
    assert.equal(value, name === 'fstatSync' ? false : undefined);
  }
  vfsState.handlers = { fstatSync() { return undefined; } };
  assert.throws(() => fs.fstatSync(rawFd, { get bigint() { throw optionError; } }), error => error === optionError);
  for (const input of ['600', 0o600]) {
    const handlers = { fchmodSync(fd, mode) { assert.equal(this, handlers); assert.equal(fd, rawFd); assert.equal(arguments.length, 2); assert.equal(mode, 0o600); return null; } };
    vfsState.handlers = handlers;
    assert.equal(fs.fchmodSync(rawFd, input, 'drop'), undefined);
  }
  let stateReads = 0;
  Object.defineProperty(vfsState, 'handlers', { configurable: true, get() { stateReads++; return {}; } });
  for (const mode of [{}, undefined]) assert.throws(() => fs.fchmodSync(rawFd, mode), { code: 'ERR_INVALID_ARG_TYPE' });
  assert.equal(stateReads, 0);
} finally {
  Object.defineProperty(vfsState, 'handlers', stateDescriptor);
  vfsState.handlers = saved;
}
console.log('SDK metadata options/catch boundary/receiver/arity/void/fd validation order passed');
