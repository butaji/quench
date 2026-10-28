'use strict';
function show(value) {
  if (typeof value === 'bigint') return `${value}n`;
  if (typeof value === 'number' && Number.isNaN(value)) return 'NaN';
  if (Object.is(value, -0)) return '-0';
  if (value && typeof value === 'object') {
    return {
      tag: value.tag, a: value.a, b: value.b, x: value.x, y: value.y,
      pairPrototype: Object.getPrototypeOf(value) === Pair.prototype,
      otherPrototype: Object.getPrototypeOf(value) === Other.prototype,
    };
  }
  return value;
}
const rows = [];
function run(name, f) {
  try { rows.push([name, show(f())]); }
  catch (error) { rows.push([name, error.name]); }
}
function Pair(a, b) { this.a = a; this.b = b; }
function Primitive() { this.x = 1; return 5; }
function ObjectReturn() { this.x = 1; return { tag: 'override', x: 2 }; }
function Other() { this.tag = 'other'; }
class Base { constructor(x) { this.x = x; } }
class Derived extends Base { constructor(x) { super(x); this.y = 2; } }
const Bound = Pair.bind(null, 3);
let proxyCalls = [];
const ProxyCtor = new Proxy(Pair, {
  construct(target, args, newTarget) {
    proxyCalls.push([args.join(','), newTarget === ProxyCtor]);
    return Reflect.construct(target, args, newTarget);
  },
});
run('new-pair', () => { const x = new Pair(4, 5); return { tag: x.a, x: x.b }; });
run('primitive-return', () => new Primitive());
run('object-return', () => new ObjectReturn());
run('bound-same-target', () => new Bound(4));
run('reflect-same-target', () => Reflect.construct(Pair, [6, 7]));
run('reflect-other-target', () => Reflect.construct(Pair, [8, 9], Other));
run('proxy', () => Reflect.construct(ProxyCtor, [10, 11]));
run('class-derived', () => new Derived(12));
run('new-arrow-error', () => new (() => {})());
run('reflect-invalid-target', () => Reflect.construct(Pair, [], () => {}));
rows.push(['proxy-events', proxyCalls]);
console.log(JSON.stringify(rows));
