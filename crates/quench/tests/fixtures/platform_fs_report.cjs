'use strict';
const fs = require('node:fs');
const os = require('node:os');
for (const bigint of [false, true]) {
  const stats = fs.statfsSync('.', { bigint });
  console.log(stats.constructor.name, typeof stats.frsize, String(stats.bsize), String(stats.frsize));
  console.log(Object.keys(stats).join(','));
  for (const name of ['bsize', 'frsize']) {
    const descriptor = Object.getOwnPropertyDescriptor(stats, name);
    console.log(name, descriptor.writable, descriptor.enumerable, descriptor.configurable);
  }
}
try {
  fs.statfsSync('__quench_missing_platform_statfs__');
} catch (error) {
  console.log(error.name, error.code, error.syscall, error.path);
}
process.report.reportOnSignal = false;
const original = process.report.signal;
const signals = ['SIGUSR2', 'SIGTERM'];
if (Object.hasOwn(os.constants.signals, 'SIGINFO')) signals.push('SIGINFO');
signals.push('sigterm', 'SIG_NO_SUCH_SIGNAL');
for (const signal of signals) {
  try {
    process.report.signal = signal;
    console.log(signal, process.report.signal);
  } catch (error) {
    console.log(signal, error.name, error.code, error.message);
  }
}
process.report.signal = original;
