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
const originalCheck = state.check;
state.check = another.check;
state.value = 9;
if (!state.check()) throw Error('callee binding lost closure environment');
record(() => state.check());
state.value = 1;
if (state.check()) throw Error('callee binding reused another closure');
record(() => state.check());
state.check = originalCheck;
state.value = 2;
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
function acrossCall(value, mutate) {
  const original = value;
  const computed = value + 1;
  mutate(arguments);
  return computed + ':' + value + ':' + original;
}
function changeArgument(args) { args[0] = 'changed'; }
for (let i = 0; i < 40; i++) {
  if (acrossCall(7, changeArgument) !== '8:changed:7') throw Error('call-edge alias recovery');
}
output.push(acrossCall(7, changeArgument));
function throughGetter(value, box) {
  const computed = value + 1;
  box.args = arguments;
  const result = box.value;
  return computed + ':' + value + ':' + result;
}
const aliasGetter = {
  get value() { this.args[0] = 'changed'; return 9; }
};
for (let i = 0; i < 40; i++) {
  if (throughGetter(7, aliasGetter) !== '8:changed:9') throw Error('getter-edge alias recovery');
}
output.push(throughGetter(7, aliasGetter));
function reflectedAfterWrite(value) {
  value = value + 1;
  return reflectedAfterWrite.arguments[0];
}
for (let i = 0; i < 40; i++) {
  if (reflectedAfterWrite(3) !== 4) throw Error('reflected return state');
}
output.push(String(reflectedAfterWrite(3)));
let phase = '';
function* deferred(value = (phase += 'p', 2)) {
  phase += 'b';
  yield value;
  phase += 'r';
  return 3;
}
const suspended = deferred();
output.push(phase);
output.push(JSON.stringify(suspended.next()));
output.push(JSON.stringify(suspended.next()));
output.push(phase);
function* invalidParameter(value = (() => { throw Error('parameter'); })()) {
  phase += 'unexpected';
  yield value;
}
record(() => invalidParameter());
output.push(phase);
function markTail(node) {
  while (node.next !== null) {
    node.mark = node.mark + 1;
    node = node.next;
  }
  return node;
}
const tailNode = {next: null, mark: 0};
const middleNode = {next: tailNode, mark: 0};
const firstNode = {next: middleNode, mark: 0};
for (let i = 0; i < 40; i++) {
  if (markTail(firstNode) !== tailNode) throw Error('typed loop identity');
}
output.push(JSON.stringify([firstNode.mark, middleNode.mark, tailNode.mark]));
middleNode.mark = 'x';
record(() => markTail(firstNode) === tailNode);
output.push(JSON.stringify([firstNode.mark, middleNode.mark, tailNode.mark]));
middleNode.mark = 2n;
record(() => markTail(firstNode));
output.push(String(firstNode.mark));
middleNode.mark = 40;
Object.defineProperty(middleNode, 'next', {
  configurable: true,
  get() { events.push('loop-next'); return tailNode; }
});
record(() => markTail(firstNode) === tailNode);
output.push(JSON.stringify([firstNode.mark, middleNode.mark, tailNode.mark]));
Object.defineProperty(middleNode, 'next', {
  get() { events.push('loop-throw'); throw TypeError('loop exit'); }
});
record(() => markTail(firstNode));
output.push(JSON.stringify([firstNode.mark, middleNode.mark, tailNode.mark]));
let edgeEntries = 0;
function enterTail(node, skip) {
  edgeEntries++;
  if (skip) return node;
  let cursor = node;
  while (cursor.next !== null) cursor = cursor.next;
  return cursor;
}
const edgeTail = {next: null};
const edgeHead = {next: edgeTail};
for (let i = 0; i < 40; i++) {
  if (enterTail(edgeHead, false) !== edgeTail) throw Error('loop edge live temporary');
  if (enterTail(edgeHead, true) !== edgeHead) throw Error('loop edge early return');
}
Object.defineProperty(edgeHead, 'next', {
  get() { events.push('edge-get'); return edgeTail; }
});
record(() => enterTail(edgeHead, false) === edgeTail);
output.push(String(edgeEntries));
let pipeMask = 2;
function pipeMark() { this.state = this.state | pipeMask; }
function pipeRun(object) { object.stamp = object.stamp + 1; object.mark(); return object.state; }
const pipeObject = {state: 0, mark: pipeMark, stamp: 0, a: 0, b: 0, c: 0};
for (let i = 0; i < 200; i++) {
  if (pipeRun(pipeObject) !== 2) throw Error('guarded pipe state');
}
output.push(String(pipeObject.state));
pipeMask = {valueOf() { events.push('pipe-coerce'); return 4; }};
record(() => pipeRun(pipeObject));
pipeMask = 2;
pipeObject.mark = function () { events.push('pipe-replacement'); this.state += 8; };
record(() => pipeRun(pipeObject));
Object.defineProperty(pipeObject, 'mark', {
  get() { events.push('pipe-get'); return pipeMark; }
});
record(() => pipeRun(pipeObject));
Object.defineProperty(pipeObject, 'state', {writable: false});
record(() => pipeRun(pipeObject));
output.push(JSON.stringify([pipeObject.state, pipeObject.stamp]));
output.push(events.join(','));
console.log(JSON.stringify(output));
