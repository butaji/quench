function C() {}
C.prototype.kind = 'C';
class D extends C {}
const direct = new C();
const manual = Object.create(C.prototype);
const derived = new D();
const deep = Object.create(Object.create(C.prototype));
let instanceTraps = 0;
const proxyInstance = new Proxy({}, { getPrototypeOf() { instanceTraps++; return C.prototype; } });
let constructorGets = 0;
const proxyConstructor = new Proxy(C, {
  get(target, key, receiver) {
    if (key === Symbol.hasInstance) constructorGets++;
    return Reflect.get(target, key, receiver);
  }
});
const custom = { [Symbol.hasInstance](value) { return value && value.kind === 'custom'; } };
const values = [
  direct instanceof C,
  manual instanceof C,
  derived instanceof C,
  derived instanceof D,
  deep instanceof C,
  proxyInstance instanceof C,
  direct instanceof proxyConstructor,
  ({ kind: 'custom' }) instanceof custom,
  ({}) instanceof custom,
  Object.create(null) instanceof C
];
Object.setPrototypeOf(direct, null);
values.push(direct instanceof C);
Object.setPrototypeOf(direct, C.prototype);
values.push(direct instanceof C, instanceTraps, constructorGets);
console.log('INSTANCEOF_ORACLE:' + JSON.stringify(values));
