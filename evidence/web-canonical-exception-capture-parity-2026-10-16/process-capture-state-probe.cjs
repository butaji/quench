'use strict';
const p = require('process');
const set = p.setUncaughtExceptionCaptureCallback;
const has = p.hasUncaughtExceptionCaptureCallback;
const out = [];
try {
  p.report.reportOnUncaughtException = true;
  set(function () {});
  out.push(['unbound', has(), p.report.reportOnUncaughtException]);
  set(null);
  out.push(['restored', has(), p.report.reportOnUncaughtException]);
} catch(e) { out.push(['error', e.name, e.code, e.message]); }
console.log(JSON.stringify(out));
