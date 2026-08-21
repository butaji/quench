const assert = require('assert');
const process = require('node:process');
assert.strictEqual(process.sourceMapsEnabled, false);
if (!process.activeResourcesInfo() || typeof process.activeResourcesInfo() !== 'object') throw new Error('active resources');
assert.strictEqual(typeof process.report.getReport, 'function');
const report = process.report.getReport();
assert.strictEqual(typeof report, 'object');
assert.strictEqual(typeof report.header, 'object');
console.log('process-yellow: ok');