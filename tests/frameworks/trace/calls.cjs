// Wrap public builtin exports and web globals; record which are called by userland.
const Module = require('node:module');
const calls = new Map();
const callerIsFramework = () => {
  const prep = Error.prepareStackTrace, lim = Error.stackTraceLimit; Error.stackTraceLimit = 6;
  Error.prepareStackTrace = (_, cs) => cs; const cs = new Error().stack; Error.prepareStackTrace = prep; Error.stackTraceLimit = lim;
  const c = cs.find(c => { const f = c.getFileName() || ''; return f && !f.endsWith('/trace/calls.cjs'); });
  const f = (c && c.getFileName()) || '';
  return f.includes('/node_modules/') || f.includes('/next-app/.next/') || f.includes('/tests/frameworks/scenarios/');
};
const hit = k => { if (callerIsFramework()) calls.set(k, (calls.get(k) || 0) + 1); };
const wrapped = new WeakSet();
function wrapFn(fn, key) {
  if (typeof fn !== 'function' || wrapped.has(fn)) return fn;
  const p = new Proxy(fn, { apply(t, th, a) { hit(key); return Reflect.apply(t, th, a); }, construct(t, a, nt) { hit(key); return Reflect.construct(t, a, nt); } });
  wrapped.add(p); return p;
}
function wrapProto(cls, key) {
  const proto = cls && cls.prototype; if (!proto || wrapped.has(proto)) return; wrapped.add(proto);
  for (const n of Object.getOwnPropertyNames(proto)) {
    if (n === 'constructor') continue;
    const d = Object.getOwnPropertyDescriptor(proto, n);
    if (d && typeof d.value === 'function' && d.configurable && d.writable) { const f = d.value; proto[n] = function (...a) { hit(key + '#' + n); return f.apply(this, a); }; }
  }
}
const skip = new Set(['module', 'process', 'sys', 'repl', 'test', 'test/reporters', 'sqlite', 'wasi', 'trace_events', 'sea', 'inspector/promises', 'punycode', 'constants', 'domain', '_stream_wrap']);
for (const name of Module.builtinModules) {
  if (name.startsWith('_') || skip.has(name)) continue;
  let m; try { m = require(name); } catch { continue; }
  if (typeof m === 'function') { wrapProto(m, name); continue; }
  for (const k of Object.keys(m)) {
    const d = Object.getOwnPropertyDescriptor(m, k); if (!d || !('value' in d) || typeof d.value !== 'function') continue;
    wrapProto(d.value, `${name}.${k}`);
    if (d.writable && d.configurable && !/^[A-Z]/.test(k)) { try { m[k] = wrapFn(d.value, `${name}.${k}`); } catch {} }
  }
}
Module.syncBuiltinESMExports();
for (const g of ['fetch', 'Request', 'Response', 'Headers', 'ReadableStream', 'WritableStream', 'TransformStream', 'TextEncoder', 'TextDecoder', 'AbortController', 'AbortSignal', 'URL', 'URLSearchParams', 'structuredClone', 'Blob', 'FormData', 'queueMicrotask', 'setImmediate', 'setTimeout', 'setInterval', 'Buffer']) {
  const v = globalThis[g]; if (typeof v !== 'function') continue; wrapProto(v, 'global.' + g);
  if (!/^[A-Z]/.test(g)) globalThis[g] = wrapFn(v, 'global.' + g);
}
process.on('exit', () => require('fs').writeFileSync(process.env.OUT, JSON.stringify(Object.fromEntries([...calls].sort()), null, 0)));
