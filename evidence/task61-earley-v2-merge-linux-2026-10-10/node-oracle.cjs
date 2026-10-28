let order = '';
function value(item) { order += item; return item * 3; }
function make() { return { first: value(1), second: value(2), third: value(3) }; }
const result = make();
console.log('literal:' + JSON.stringify([order, Object.keys(result).join(','), result.first + ',' + result.second + ',' + result.third, Object.getOwnPropertyDescriptor(result, 'second').enumerable]));
let observed = -1;
Object.defineProperty(Array.prototype, '5', { configurable: true, set(v) { observed = v; } });
const setterArray = [];
setterArray[5] = 17;
console.log('setter:' + JSON.stringify([observed, Object.prototype.hasOwnProperty.call(setterArray, '5')]));
delete Array.prototype[5];
const prototype = [41];
const indexed = [];
Object.setPrototypeOf(indexed, prototype);
indexed[0] = 9;
console.log('indexed:' + JSON.stringify([indexed[0], prototype[0], indexed.hasOwnProperty(0)]));
let getterCalls = 0;
const inherited = { value: 1 };
const receiver = Object.create(inherited);
function read(obj) { return obj.value; }
const inheritedValues = [read(receiver), read(receiver)];
inherited.value = 2;
inheritedValues.push(read(receiver));
Object.defineProperty(inherited, 'value', { configurable: true, get() { getterCalls++; return this === receiver ? getterCalls + 2 : -1; } });
inheritedValues.push(read(receiver), read(receiver), getterCalls);
const next = { value: 8 };
Object.setPrototypeOf(receiver, next);
inheritedValues.push(read(receiver));
receiver.value = 9;
inheritedValues.push(read(receiver));
console.log('prototype-cache:' + JSON.stringify(inheritedValues));
function C() {}
class D extends C {}
let proxyTraps = 0;
const proxyValue = new Proxy({}, { getPrototypeOf() { proxyTraps++; return C.prototype; } });
console.log('instanceof:' + JSON.stringify([new C() instanceof C, new D() instanceof C, Object.create(Object.create(C.prototype)) instanceof C, proxyValue instanceof C, proxyTraps]));
console.log('numeric:' + JSON.stringify([1 + 2, 1.5 + 2.25, 3 * 4, -0 * 2, 1 / 0]));
