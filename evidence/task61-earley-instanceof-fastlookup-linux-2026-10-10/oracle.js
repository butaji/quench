const out = [];
function record(name, fn) { try { out.push([name, 'return', fn()]); } catch (e) { out.push([name, 'throw', e.name, e.message]); } }
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
record('custom_false', () => ({} instanceof A));
out.push(['custom_call_count', customCalls]);
Object.defineProperty(A, Symbol.hasInstance, { configurable: true, get() { out.push(['hasinstance_getter']); return function (value) { return value === a; }; } });
record('getter_custom', () => a instanceof A);
function B() {}
Object.defineProperty(B, Symbol.hasInstance, { value: 7, configurable: true });
record('noncallable_hasinstance', () => a instanceof B);
function C() {}
const protoLog = [];
const customFunctionPrototype = Object.create(Function.prototype);
Object.defineProperty(customFunctionPrototype, Symbol.hasInstance, { configurable: true, value(value) { protoLog.push(value === marker); return value === marker; } });
Object.setPrototypeOf(C, customFunctionPrototype);
const marker = {};
const customProtoCheck = value => value instanceof C;
record('custom_function_prototype_true', () => customProtoCheck(marker));
record('custom_function_prototype_false', () => customProtoCheck({}));
out.push(['custom_function_prototype_log', protoLog]);
const proxyLog = [];
function E() {}
const Ep = new Proxy(E, { get(target, key, receiver) { proxyLog.push(String(key)); if (key === Symbol.hasInstance) return undefined; return Reflect.get(target, key, receiver); } });
record('proxy_ctor_instance', () => new E() instanceof Ep);
out.push(['proxy_log', proxyLog]);
function F() {}
const Fp = new Proxy(F, { get(target, key, receiver) { if (key === Symbol.hasInstance) out.push(['proxy_get_hasinstance']); return Reflect.get(target, key, receiver); } });
record('proxy_default_method', () => new F() instanceof Fp);
function D() {}
const BoundD = D.bind(null);
record('bound_instance', () => new D() instanceof BoundD);
record('bound_noninstance', () => ({}) instanceof BoundD);
console.log(JSON.stringify(out));
