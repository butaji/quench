'use strict';
const p = require('process');
const set = p.setUncaughtExceptionCaptureCallback;
const has = p.hasUncaughtExceptionCaptureCallback;
const out = [];
for (const receiver of [undefined, null, {}, p]) {
  try { set.call(receiver, function () {}); out.push(['set', has.call(receiver), p.hasUncaughtExceptionCaptureCallback()]); set.call(receiver, null); out.push(['clear',has.call(receiver),p.hasUncaughtExceptionCaptureCallback()]); }
  catch(e) { out.push(['error',e.name,e.code,e.message]); p.setUncaughtExceptionCaptureCallback(null); }
}
console.log(JSON.stringify(out));
