// Node compat: v8.getHeapSpaceStatistics returns Node-shaped records.
const v8 = require('node:v8');

if (typeof v8.getHeapSpaceStatistics !== 'function') {
  throw new Error('no getHeapSpaceStatistics');
}
const spaces = v8.getHeapSpaceStatistics();
if (!Array.isArray(spaces) || spaces.length === 0) {
  throw new Error('heap spaces must be a non-empty array');
}
for (const space of spaces) {
  for (const key of [
    'space_name',
    'space_size',
    'space_used_size',
    'space_available_size',
    'physical_space_size',
  ]) {
    if (!(key in space)) throw new Error('missing heap space field: ' + key);
  }
  if (typeof space.space_name !== 'string') throw new Error('invalid space name');
  for (const key of [
    'space_size',
    'space_used_size',
    'space_available_size',
    'physical_space_size',
  ]) {
    if (!Number.isInteger(space[key]) || space[key] < 0) {
      throw new Error('invalid heap space value: ' + key);
    }
  }
}

console.log('v8-heap-space-statistics: ok');
