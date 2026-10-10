function Pair(a, b) {
  this.car = a;
  this.cdr = b;
}
function Args(...values) {
  this.values = values;
  this.target = new.target.name;
}
function ReturnsObject(value) {
  this.ignored = true;
  return { returned: value };
}
class Derived extends Args {}

const Bound = Args.bind(null, 'bound');
const ProxyArgs = new Proxy(Args, {
  construct(target, args, newTarget) {
    return Reflect.construct(target, args, newTarget);
  }
});
const result = [
  new Pair(),
  new Pair(1, 2),
  new Args(),
  new Args(1, 2),
  new Args(0, 1, 2, 3, 4, 5, 6, 7),
  new Args(0, 1, 2, 3, 4, 5, 6, 7, 8),
  new Derived('derived', 9),
  new Bound('tail'),
  new ProxyArgs('proxy', 10),
  new ReturnsObject('override')
].map(value => ({
  values: value.values || [value.car, value.cdr],
  target: value.target || '',
  returned: value.returned || ''
}));
const output = 'CONSTRUCT_ARGS_ORACLE:' + JSON.stringify(result);
if (typeof print === 'function') print(output);
else console.log(output);
