// Node compat: process module.
const process = require('node:process');
if (process.version.length === 0) throw new Error('no version');
if (process.platform.length === 0) throw new Error('no platform');
if (process.arch.length === 0) throw new Error('no arch');
if (process.pid <= 0) throw new Error('pid=' + process.pid);
if (process.argv.length === 0) throw new Error('argv empty');
if (typeof process.cwd() !== 'string') throw new Error('cwd');
console.log('process: %s %s %d', process.platform, process.arch, process.pid);
