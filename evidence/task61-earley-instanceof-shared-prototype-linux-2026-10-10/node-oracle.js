'use strict';
const rows = [];
const events = [];
function show(value) {
  if (typeof value === 'number' && Number.isNaN(value)) return 'NaN';
  if (Object.is(value, -0)) return '-0';
  if (value && typeof value === 'object') return { tag: value.tag };
  return value;
}
function run(name, action) {
  try { rows.push([name, show(action())]); }
  catch (error) { rows.push([name, error.name]); }
}
function C() {}
const instance = new C();
run('ordinary-true', () => instance instanceof C);
run('ordinary-false', () => ({}) instanceof C);
run('primitive-default', () => 3 instanceof C);
const oldPrototype = C.prototype;
const nextPrototype = { tag: 'next' };
C.prototype = nextPrototype;
run('mutated-prototype-old-instance', () => instance instanceof C);
run('mutated-prototype-new-chain', () => Object.create(nextPrototype) instanceof C);
C.prototype = oldPrototype;
const proxyInstance = new Proxy({}, {
  getPrototypeOf(target) { events.push(['instance-get-prototype']); return C.prototype; },
});
run('proxy-instance', () => proxyInstance instanceof C);
const sentinel = { tag: 'thrown' };
const throwingProxy = new Proxy({}, { getPrototypeOf() { throw sentinel; } });
let thrownIdentity = false;
try { void (throwingProxy instanceof C); } catch (error) { thrownIdentity = error === sentinel; }
rows.push(['proxy-throw-identity', thrownIdentity]);
const proxyCtor = new Proxy(C, {
  get(target, key, receiver) {
    if (key === Symbol.hasInstance) events.push(['constructor-get-hasInstance']);
    if (key === 'prototype') events.push(['constructor-get-prototype']);
    return Reflect.get(target, key, receiver);
  },
});
run('proxy-constructor', () => instance instanceof proxyCtor);
function Custom() {}
Object.defineProperty(Custom, Symbol.hasInstance, {
  get() { events.push(['custom-get']); return function (value) { events.push(['custom-call', value === instance, this === Custom]); return 2; }; },
});
run('custom-has-instance', () => instance instanceof Custom);
const objectConstructor = {
  [Symbol.hasInstance](value) { events.push(['object-method', value === instance]); return 'yes'; },
};
run('object-has-instance', () => instance instanceof objectConstructor);
const badConstructor = { [Symbol.hasInstance]: 1 };
run('bad-has-instance', () => instance instanceof badConstructor);
const revoked = Proxy.revocable({}, {});
revoked.revoke();
run('revoked-instance-proxy', () => revoked.proxy instanceof C);
class Base {}
class Derived extends Base {}
run('class-chain', () => new Derived() instanceof Base);
rows.push(['events', events]);
console.log(JSON.stringify(rows));
