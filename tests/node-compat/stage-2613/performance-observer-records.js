const assert = require('assert');
const { performance, PerformanceObserver } = require('perf_hooks');

let callbackRecords = 0;
const observer = new PerformanceObserver((list) => {
  callbackRecords += list.getEntries().length;
});
observer.observe({ entryTypes: ['mark'] });
performance.mark('observer-mark');
assert.strictEqual(callbackRecords, 1);
assert.strictEqual(observer.takeRecords().length, 1);
assert.strictEqual(observer.takeRecords().length, 0);
observer.disconnect();
console.log('performance observer records: ok');
