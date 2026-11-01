const output = [], events = [];
function calc(o) {
  const x = o.left, y = o.right;
  const sum = x + y;
  return sum * 3;
}
function nested(o) {
  const x = o.child.left, y = o.child.right;
  const sum = x + y;
  return sum * 3;
}
function method(n) {
  const x = this.left, y = this.right;
  const sum = x + y;
  return sum + n;
}
function reassigned(o, other) {
  const x = o.left;
  o = other;
  const y = o.right, sum = x + y;
  return sum * 3;
}
function alias(o, other) {
  const x = o.left, y = o.right, sum = x + y;
  other.right = 7;
  return sum + o.right;
}
function make(left, right) { return {pad: 0, left, right}; }
function record(action) {
  try {
    const value = action();
    output.push(value !== value ? 'NaN' : Object.is(value, -0) ? '-0' : String(value));
  } catch (error) { output.push(error.name); }
}
const box = make(2, 3), root = {pad: 0, child: box, tail: 0};
for (let i = 0; i < 40; i++) {
  if (calc(box) !== 15 || nested(root) !== 15 || method.call(box, 1) !== 6) throw Error('warm fields');
}
record(() => calc(box));
record(() => nested(root));
root.child = {right: 6, pad: 0, left: 5};
record(() => nested(root));
record(() => method.call(box, 1));
record(() => method.call(2, 1));
record(() => reassigned(box, {right: 11, left: 99, pad: 0}));
record(() => alias(box, box));
for (const pair of [[-0, -0], [0.5, 1.25], [NaN, 1], [Infinity, -Infinity], ['2', 3], [2n, 3n]]) {
  record(() => calc(make(pair[0], pair[1])));
}
const hooked = make({valueOf() { events.push('coerce'); hooked.right = 9; return 2; }}, 3);
record(() => calc(hooked));
output.push(String(hooked.right));
record(() => calc({pad: 0, get left() { events.push('left'); return 2; }, get right() { events.push('right'); return 3; }}));
const changing = make(2, 3);
calc(changing);
Object.defineProperty(changing, 'right', {get() { events.push('changed'); return 8; }});
record(() => calc(changing));
record(() => calc(new Proxy(make(2, 3), {get(target, key, receiver) {
  events.push(key); return Reflect.get(target, key, receiver);
}})));
const prototype = make(4, 5), inherited = Object.create(prototype);
record(() => calc(inherited));
record(() => calc(inherited));
prototype.left = '4';
record(() => calc(inherited));
Object.setPrototypeOf(inherited, make(6, 7));
record(() => calc(inherited));
root.child = null;
record(() => nested(root));
function makeState(mask, expected) {
  return {
    value: 0, pad: 0, tail: 0,
    check() { return (this.value & mask) !== 0 || this.value == expected; },
    mark() { this.value = this.value | mask; },
    assign() { this.value = expected; },
    change(nextMask, nextExpected) { mask = nextMask; expected = nextExpected; }
  };
}
const state = makeState(2, 7), another = makeState(4, 9);
for (let i = 0; i < 40; i++) {
  state.value = 0; state.mark();
  if (!state.check()) throw Error('warm guarded calls');
  another.assign();
  if (another.value !== 9) throw Error('closure-specific store');
  if (!another.check()) throw Error('closure-specific guarded calls');
}
record(() => state.check());
state.assign(); output.push(String(state.value));
state.change('2', 7); state.value = '1'; state.mark();
output.push(String(state.value));
record(() => state.check());
const identity = {valueOf() { events.push('guarded-coerce'); return 2; }};
state.change(2, identity); state.assign();
output.push(String(state.value === identity));
record(() => state.check());
Object.defineProperty(state, 'value', {
  get() { events.push('guarded-get'); return 0; },
  set(value) { events.push('guarded-set:' + value); }
});
state.change(2, 7);
record(() => state.check()); state.mark(); state.assign();
output.push(events.join(','));
console.log(JSON.stringify(output));
