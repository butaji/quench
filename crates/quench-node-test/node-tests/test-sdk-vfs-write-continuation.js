'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { vfsState } = require('internal/fs/utils');
const savedState = vfsState.handlers;
const savedMethods = Object.fromEntries(['openSync', 'writeSync', 'fsyncSync', 'closeSync'].map(name => [name, fs[name]]));
try {
  let log = [];
  const parent = { get inherited() { log.push('inherited'); return 7; } };
  const options = Object.assign(Object.create(parent), {
    get encoding() { return 'utf8'; },
  });
  Object.defineProperties(options, {
    encoding: { enumerable: true, configurable: true, get() { log.push('encoding'); return 'utf8'; } },
    mode: { enumerable: true, get() { log.push('mode'); return '600'; } },
    flag: { enumerable: true, get() { log.push('flag'); return 'a'; } },
  });
  const handlers = {
    appendFileSync(path, data, actual) { log.push('append'); assert.equal(actual, options); return undefined; },
    writeFileSync(path, data, actual) {
      log.push('write'); assert.notEqual(actual, options);
      assert.equal(Object.hasOwn(actual, 'inherited'), true); assert.equal(actual.inherited, 7);
      return false;
    },
  };
  vfsState.handlers = handlers;
  fs.appendFileSync('guest-path', 'raw', options);
  assert.deepEqual(log, ['encoding', 'encoding', 'mode', 'append', 'encoding', 'mode', 'flag', 'inherited', 'write']);

  log = [];
  const bytes = Buffer.from('abc');
  const modeOptions = {
    encoding: 'utf8', flag: 'w', flush: true,
    get mode() { log.push('mode'); return '600'; },
  };
  fs.openSync = function(path, flag, mode) { log.push(['open', path, flag, mode, arguments.length]); return 55; };
  fs.writeSync = function(fd, data, offset, length) {
    assert.equal(data, bytes); log.push(['write', fd, offset, length, arguments.length]); return 1;
  };
  fs.fsyncSync = function(fd) { log.push(['fsync', fd, arguments.length]); };
  fs.closeSync = function(fd) { log.push(['close', fd, arguments.length]); };
  vfsState.handlers = { writeFileSync() { log.push('handler'); return undefined; } };
  fs.writeFileSync('guest-path', bytes, modeOptions);
  assert.deepEqual(log, ['mode', 'handler', 'mode', ['open', 'guest-path', 'w', '600', 3], ['write', 55, 0, 3, 4], ['write', 55, 1, 2, 4], ['write', 55, 2, 1, 4], ['fsync', 55, 1], ['close', 55, 1]]);

  log = [];
  fs.writeFileSync(12, bytes, { flush: true });
  assert.deepEqual(log, ['handler', ['write', 12, 0, 3, 4], ['write', 12, 1, 2, 4], ['write', 12, 2, 1, 4], ['fsync', 12, 1]]);
  const writeError = {}, closeError = {};
  fs.writeSync = () => { throw writeError; };
  fs.closeSync = () => { throw closeError; };
  assert.throws(() => fs.writeFileSync('guest-path', bytes), error => error === closeError);
} finally {
  vfsState.handlers = savedState;
  Object.assign(fs, savedMethods);
}
console.log('SDK append copy/getters and write loop/fd/flush/finally continuation passed');
