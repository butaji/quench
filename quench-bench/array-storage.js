// Guest semantic oracle for dense-array storage and named-property transitions.
// Run unchanged under Node and Quench; this is outside the frozen micro edition.
const records = [];
const symbol = Symbol('named');
function value(v) {
  if (v === undefined) return 'undefined';
  if (typeof v === 'symbol') return String(v);
  if (typeof v === 'function') return 'function';
  if (Object.is(v, -0)) return '-0';
  return v;
}
function observe(label, a) {
  records.push([label, Array.isArray(a), a.length,
    Reflect.ownKeys(a).map(k => {
      const d = Object.getOwnPropertyDescriptor(a, k);
      return [String(k), value(d.value), d.writable, d.enumerable,
        d.configurable, !!d.get, !!d.set];
    }), Array.from({length: Math.min(a.length, 6)}, (_, i) => [i in a, value(a[i])])]);
}
function literal() { return [1, 2, 3]; }
for (const extrasFirst of [false, true]) {
  const a = literal(), untouched = literal(), sliced = a.slice();
  if (extrasFirst) Object.defineProperty(a, 'hidden', {value: 7, configurable: true});
  a.named = 11;
  a[symbol] = 13;
  a[0] = -0;
  a.push(4);
  delete a[1];
  observe('mutated:' + extrasFirst, a);
  observe('literal:' + extrasFirst, untouched);
  observe('slice:' + extrasFirst, sliced);
  delete a.named;
  delete a[symbol];
  Object.defineProperty(a, 'length', {value: 2, writable: false});
  records.push(['append-fails', Reflect.set(a, '2', 8), Reflect.set(a, 'named', 17)]);
  observe('fixed-length:' + extrasFirst, a);
  Object.freeze(a);
  records.push(['frozen', Object.isFrozen(a), Reflect.set(a, '0', 19)]);
  observe('frozen:' + extrasFirst, a);
}
const prototype = Object.create(Array.prototype);
Object.defineProperty(prototype, '1', {get() { return 29; }, configurable: true});
const holes = [5, , 7];
Object.setPrototypeOf(holes, prototype);
observe('prototype-hole', holes);
Object.defineProperty(holes, '1', {value: 31, writable: true, enumerable: true, configurable: true});
holes.named = 37;
observe('own-index', holes);
const sparse = [];
sparse[100000] = 41;
sparse.named = 43;
records.push(['sparse', sparse.length, sparse[100000], sparse.named, Object.keys(sparse)]);
sparse.length = 0;
observe('sparse-truncated', sparse);
class Derived extends Array { #secret = 47; secret() { return this.#secret; } }
const derived = new Derived(2);
derived[0] = 53;
derived.named = 59;
observe('derived', derived);
records.push(['private', derived.secret(), derived instanceof Derived]);
console.log(JSON.stringify(records));
