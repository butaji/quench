'use strict';
const assert = require('node:assert/strict');
const signals = require('node:os').constants.signals;
const report = process.report;
const original = report.signal;
const descriptor = Object.getOwnPropertyDescriptor(report, 'signal');
const errors = [];
try {
  for (const name of Object.keys(signals)) {
    report.signal = name;
    assert.equal(report.signal, name);
  }
  for (const name of ['sigterm', 'SIGUNKNOWN',
    ...(process.platform === 'darwin' ? ['SIGSTKFLT', 'SIGPOLL', 'SIGPWR'] : ['SIGINFO'])]) {
    const previous = report.signal;
    assert.throws(() => { report.signal = name; }, error => {
      errors.push([name, error.name, error.code, error.message]);
      return error instanceof TypeError && error.code === 'ERR_UNKNOWN_SIGNAL';
    });
    assert.equal(report.signal, previous);
  }
} finally {
  report.signal = original;
}
console.log(JSON.stringify({signals, errors, descriptor: {
  get: typeof descriptor.get, set: typeof descriptor.set,
  enumerable: descriptor.enumerable, configurable: descriptor.configurable,
}}));
