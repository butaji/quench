// Node compat: inspector + trace_events shape.
const inspector = require('node:inspector');
const te = require('node:trace_events');
if (typeof inspector !== 'object') throw new Error('inspector: ' + typeof inspector);
if (typeof te !== 'object') throw new Error('trace_events: ' + typeof te);
console.log('inspector+te: ok');
