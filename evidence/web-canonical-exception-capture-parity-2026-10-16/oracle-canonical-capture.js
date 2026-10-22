'use strict';
const p = require('process');
const set = p.setUncaughtExceptionCaptureCallback;
const has = p.hasUncaughtExceptionCaptureCallback;
const out = [];
for (const receiver of [undefined, null, {}, p]) {
  set.call(receiver, function () {});
  out.push(['set', has.call(receiver), has(), p.hasUncaughtExceptionCaptureCallback()]);
  if (global.gc) global.gc();
  set.call(receiver, null);
  out.push(['clear', has.call(receiver), has(), p.hasUncaughtExceptionCaptureCallback()]);
}
const forged = String.fromCharCode(0) + 'quench:process-capture-callback';
p[forged] = () => {};
out.push(['forged', has()]);
delete p[forged];
set(function capture(error) {
  out.push(['capture', this === p, Object.keys(this), this.captureFn === capture, error.message]);
  this.captureFn = null;
  out.push(['mutated',has()]);
  if (global.gc) global.gc();
  console.log(JSON.stringify(out));
});
process.nextTick(() => { throw new Error('capture receiver'); });
