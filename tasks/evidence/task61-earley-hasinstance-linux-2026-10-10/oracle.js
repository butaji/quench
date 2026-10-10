const out = [];
function record(name, fn) { try { const value = fn(); out.push([name, 'return', value]); } catch (e) { out.push([name, 'throw', e.name, e.message]); } }
function A() {}
const a = new A();
record('ordinary_true', () => a instanceof A);
record('ordinary_false', () => ({}) instanceof A);
record('primitive_left', () => 1 instanceof A);
record('invalid_rhs', () => 1 instanceof 1);
record('null_rhs', () => 1 instanceof null);
let customCalls = 0;
Object.defineProperty(A, Symbol.hasInstance, { configurable: true, value: function (value) { customCalls++; return value === a; } });
record('custom_true', () => a instanceof A);
record('custom_false', () => ({}) instanceof A);
out.push(['custom_call_count', customCalls]);
Object.defineProperty(A, Symbol.hasInstance, { configurable: true, get() { out.push(['hasinstance_getter']); return function (value) { return value === a; }; } });
record('getter_custom', () => a instanceof A);
function B() {}
Object.defineProperty(B, Symbol.hasInstance, { value: 7, configurable: true });
record('noncallable_hasinstance', () => a instanceof B);
function C() {}
const Cp = new Proxy(C, { get(target, key, receiver) { if (key === 'prototype') { out.push(['prototype_getter']); return {}; } return Reflect.get(target, key, receiver); } });
record('prototype_getter_true', () => ({}) instanceof Cp);
function D() {}
const BoundD = D.bind(null);
record('bound_instance', () => new D() instanceof BoundD);
record('bound_noninstance', () => ({}) instanceof BoundD);
const proxyLog = [];
function E() {}
const Ep = new Proxy(E, { get(target, key, receiver) { proxyLog.push(String(key)); if (key === Symbol.hasInstance) return undefined; return Reflect.get(target, key, receiver); } });
record('proxy_ctor_instance', () => new E() instanceof Ep);
out.push(['proxy_log', proxyLog]);
const methodProxyLog = [];
function F() {}
const Fp = new Proxy(F, { get(target, key, receiver) { if (key === Symbol.hasInstance) methodProxyLog.push('get_hasinstance'); return Reflect.get(target, key, receiver); } });
record('proxy_default_method', () => new F() instanceof Fp);
out.push(['proxy_method_log', methodProxyLog]);
console.log(JSON.stringify(out));
