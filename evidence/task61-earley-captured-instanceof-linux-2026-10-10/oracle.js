const events = [];
const rows = [];
function record(name, fn) {
  try { rows.push([name, 'return', fn()]); }
  catch (error) { rows.push([name, 'throw', error.name, error.message]); }
}
function A() {}
function B() {}
function capturedCheck(value, constructor) { return value instanceof constructor; }
record('ordinary_true', () => capturedCheck(new A(), A));
record('ordinary_false', () => capturedCheck({}, A));
record('primitive_left', () => capturedCheck(1, A));
let current = A;
const mutableCheck = value => value instanceof current;
record('capture_before_mutation', () => mutableCheck(new A()));
current = B;
record('capture_after_mutation', () => mutableCheck(new B()));
const custom = function Custom() {};
Object.defineProperty(custom, Symbol.hasInstance, { configurable: true, value(value) { events.push(['custom', value === marker]); return value === marker; } });
const marker = {};
const customCheck = value => value instanceof custom;
record('custom_true', () => customCheck(marker));
record('custom_false', () => customCheck({}));
function Throwing() {}
Object.defineProperty(Throwing, Symbol.hasInstance, { configurable: true, value() { events.push(['throwing']); throw new RangeError('oracle boom'); } });
const throwingCheck = value => value instanceof Throwing;
record('custom_throw', () => throwingCheck({}));
function Base() {}
const proxyLog = [];
const proxy = new Proxy(Base, { get(target, key, receiver) { if (key === Symbol.hasInstance) proxyLog.push('hasInstance'); if (key === 'prototype') proxyLog.push('prototype'); return Reflect.get(target, key, receiver); } });
const proxyCheck = value => value instanceof proxy;
record('proxy_true', () => proxyCheck(new Base()));
rows.push(['proxy_log', proxyLog]);
function BoundTarget() {}
const bound = BoundTarget.bind(null);
const boundCheck = value => value instanceof bound;
record('bound_true', () => boundCheck(new BoundTarget()));
record('bound_false', () => boundCheck({}));
let tdzCheck;
(function () {
  tdzCheck = value => value instanceof Late;
  record('capture_tdz', () => tdzCheck({}));
  let Late = function Late() {};
  record('capture_after_tdz', () => tdzCheck(new Late()));
})();
rows.push(['events', events]);
console.log(JSON.stringify(rows));
