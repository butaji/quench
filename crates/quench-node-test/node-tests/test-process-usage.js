const assert = require('assert');
const cpu = process.cpuUsage();
assert.strictEqual(typeof cpu.user, 'number');
assert.strictEqual(typeof cpu.system, 'number');
const usage = process.resourceUsage();
for (const key of ['userCPUTime', 'systemCPUTime', 'maxRSS', 'fsRead', 'fsWrite']) {
  assert.strictEqual(typeof usage[key], 'number', key);
}
