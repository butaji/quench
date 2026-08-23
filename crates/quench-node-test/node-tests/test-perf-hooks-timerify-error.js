// Regression: timerify preserves synchronous errors and remains callable.
const { timerify } = require('node:perf_hooks');
const wrapped = timerify(() => {
  throw new Error('test');
});
let thrown;
try {
  wrapped();
} catch (error) {
  thrown = error;
}
if (!thrown || thrown.message !== 'test') throw new Error('timerify error propagation');
console.log('perf_hooks timerify error: ok');
