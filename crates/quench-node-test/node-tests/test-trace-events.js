const trace = require('node:trace_events');
if (typeof trace.createTracing !== 'function') throw new Error('createTracing');
if (typeof trace.Tracing !== 'function' && typeof trace.Tracing !== 'object') throw new Error('Tracing');
if (trace.getEnabledCategories() !== '') throw new Error('initial categories');
const t = trace.createTracing({ categories: ['node.test', 'v8'] });
if (t.categories !== 'node.test,v8' || t.enabled) throw new Error('initial tracing');
t.enable();
if (!t.enabled || trace.getEnabledCategories() !== 'node.test,v8') throw new Error('enable');
t.disable();
if (t.enabled || trace.getEnabledCategories() !== '') throw new Error('disable');
let threw = false;
try { trace.createTracing({ categories: 'node.test' }); } catch (_) { threw = true; }
if (!threw) throw new Error('validation');
console.log('trace_events: ok');
