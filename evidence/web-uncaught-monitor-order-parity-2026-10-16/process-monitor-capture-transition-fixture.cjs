'use strict';
const out=[];
process.on('uncaughtExceptionMonitor', (error, origin) => {
  out.push(['monitor',error.message,origin]);
  if (error.message === 'second') process.setUncaughtExceptionCaptureCallback(replacement);
});
function replacement(error) {
  out.push(['replacement',error.message]);
  process.setUncaughtExceptionCaptureCallback(null);
  process.nextTick(() => { throw new Error('third'); });
}
process.on('uncaughtException', (error, origin) => {
  out.push(['uncaught',error.message,origin]);
  console.log(JSON.stringify(out));
});
process.setUncaughtExceptionCaptureCallback(function (error) {
  out.push(['capture',error.message]);
  process.setUncaughtExceptionCaptureCallback(null);
  process.nextTick(() => { throw new Error('second'); });
});
process.nextTick(() => { throw new Error('first'); });
