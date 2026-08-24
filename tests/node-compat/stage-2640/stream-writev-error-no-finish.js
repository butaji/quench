'use strict';
const common = require('../../node/test/common');
const { Writable } = require('stream');

function exercise(schedule) {
  const writable = new Writable();
  writable._write = (chunk, encoding, callback) => callback(new Error('write test error'));
  writable._writev = (chunks, callback) => schedule(callback, new Error('writev test error'));
  writable.on('finish', common.mustNotCall());
  writable.on('prefinish', common.mustNotCall());
  writable.on('error', common.mustCall((error) => {
    if (error.message !== 'writev test error') throw new Error(error.message);
  }));
  writable.cork();
  writable.write('test');
  setImmediate(() => writable.end('test'));
}

exercise((callback, error) => callback(error));
exercise((callback, error) => setImmediate(callback, error));
