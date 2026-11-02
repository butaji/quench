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
function terminalPipe(object, value) {
  object.seen = object.seen + 1;
  return object.finish(value);
}
const terminalObject = {seen: 0, finish(value) { return this.seen + value; }, a: 0, b: 0, c: 0};
for (let i = 0; i < 200; i++) {
  if (terminalPipe(terminalObject, 3) !== i + 4) throw Error('terminal pipe state');
}
terminalObject.finish = function (value) { events.push('terminal-call'); return value + this.seen; };
record(() => terminalPipe(terminalObject, 7));
Object.defineProperty(terminalObject, 'finish', {
  get() { events.push('terminal-get'); return function () { throw Error('terminal-error:' + this.seen); }; }
});
record(() => terminalPipe(terminalObject, 9));
output.push(String(terminalObject.seen));
function headerPivot(object) {
  const first = object.left;
  object = object.next;
  const second = object.left;
  object.mark();
  return first + second;
}
const headerTail = {a: 0, left: 5, mark() { this.a++; }, b: 0, c: 0};
const headerHead = {left: 2, next: headerTail, a: 0, b: 0, c: 0};
for (let i = 0; i < 200; i++) {
  if (headerPivot(headerHead) !== 7) throw Error('header redefinition');
}
Object.defineProperty(headerTail, 'left', {get() { events.push('header-get'); return 9; }});
record(() => headerPivot(headerHead));
output.push(String(headerTail.a));
function makeBound(mask) {
  const object = {state: 0, stamp: 0, changeMask(value) { mask = value; }, a: 0, b: 0, c: 0};
  object.uniqueBoundMark = function () { this.state = this.state | mask; };
  return object;
}
function runBound(object) { object.stamp = object.stamp + 1; object.uniqueBoundMark(); return object.state; }
const boundFirst = makeBound(1);
const boundSecond = makeBound(2);
for (let i = 0; i < 200; i++) {
  if (runBound(boundFirst) !== 1 || runBound(boundSecond) !== 2) throw Error('dynamic closure capture');
}
boundFirst.changeMask(4);
record(() => runBound(boundFirst));
boundFirst.changeMask({valueOf() { events.push('bound-coerce'); return 8; }});
record(() => runBound(boundFirst));
Object.defineProperty(boundSecond, 'uniqueBoundMark', {
  value() { events.push('bound-replacement'); this.state += 16; }
});
record(() => runBound(boundSecond));
Object.defineProperty(boundFirst, 'state', {writable: false});
record(() => runBound(boundFirst));
output.push(JSON.stringify([boundFirst.state, boundFirst.stamp, boundSecond.state, boundSecond.stamp]));
function makePure(mask) {
  const object = {state: 3, stamp: 0, changeMask(value) { mask = value; }, a: 0, b: 0, c: 0};
  object.pureBoundProbe = function (expected) { return (this.state & mask) === expected; };
  return object;
}
function runPure(object, expected) {
  object.stamp = object.stamp + 1;
  const result = object.pureBoundProbe(expected);
  if (result) return 11;
  return 12;
}
const pureFirst = makePure(1);
const pureSecond = makePure(2);
for (let i = 0; i < 200; i++) {
  if (runPure(pureFirst, 1) !== 11 || runPure(pureSecond, 1) !== 12) throw Error('pure closure arguments');
}
for (const expected of [NaN, Infinity, -Infinity, -0, 0, 1, 1.5, 1n, undefined, null, '1',
  {valueOf() { events.push('unexpected-pure-coerce'); return 1; }}]) {
  record(() => runPure(pureFirst, expected));
}
pureFirst.changeMask(1n);
record(() => runPure(pureFirst, 1));
pureFirst.changeMask({valueOf() { events.push('pure-coerce'); return 1; }});
record(() => runPure(pureFirst, 1));
Object.defineProperty(pureSecond, 'state', {get() { events.push('pure-get'); return 1; }});
record(() => runPure(pureSecond, 0));
Object.defineProperty(pureFirst, 'pureBoundProbe', {value(expected) { events.push('pure-replacement'); return expected; }});
record(() => runPure(pureFirst, 0));
output.push(JSON.stringify([pureFirst.stamp, pureSecond.stamp]));
function fusedRoute(base) {
  this.stamp = this.stamp + 1;
  this.current = (base = base.next.value);
  this.finish();
  return base;
}
const fusedTail = {value: 7, a: 0, b: 0, c: 0, d: 0, e: 0};
const fusedBase = {next: fusedTail, a: 0, b: 0, c: 0, d: 0, e: 0};
const fusedOwner = {current: 0, stamp: 0, route: fusedRoute, finish() {}, a: 0, b: 0, c: 0};
for (let i = 0; i < 200; i++) {
  if (fusedOwner.route(fusedBase) !== 7) throw Error('fused alias destination');
}
Object.defineProperty(fusedTail, 'value', {get() { events.push('fused-second'); return 9; }});
record(() => fusedOwner.route(fusedBase));
Object.defineProperty(fusedOwner, 'current', {set(value) { events.push('fused-sink:' + value); }, configurable: true});
record(() => fusedOwner.route(fusedBase));
Object.defineProperty(fusedBase, 'next', {get() { events.push('fused-first'); return fusedTail; }});
record(() => fusedOwner.route(fusedBase));
Object.defineProperty(fusedOwner, 'current', {value: 3, writable: false});
record(() => fusedOwner.route(fusedBase));
output.push(JSON.stringify([fusedOwner.current, fusedOwner.stamp]));
function fusedWalk(node) {
  this.stamp = this.stamp + 1;
  this.current = node;
  while (this.current !== null) {
    this.current.uniqueBoundMark();
    this.current = this.current.next;
  }
  return this.stamp;
}
const walkFirst = makeBound(1), walkSecond = makeBound(2);
walkFirst.next = walkSecond;
walkSecond.next = null;
const walkOwner = {stamp: 0, current: null, walk: fusedWalk, a: 0, b: 0, c: 0};
for (let i = 0; i < 200; i++) {
  if (walkOwner.walk(walkFirst) !== 1 + i) throw Error('loop prefix replay');
}
walkFirst.changeMask({valueOf() { events.push('walk-coerce'); return 4; }});
record(() => walkOwner.walk(walkFirst));
output.push(JSON.stringify([walkOwner.stamp, walkFirst.state, walkSecond.state]));
const exactPrototype = {};
exactPrototype.consumeExact = function (input) {
  this.count = this.count + 1;
  const value = input.value;
  return value + this.count;
};
function runExact(owner, input) {
  const value = owner.consumeExact(input);
  return value + 3;
}
const exactOwner = Object.create(exactPrototype);
exactOwner.count = 0;
const exactInput = {value: 7};
for (let i = 0; i < 100; i++) {
  if (runExact(exactOwner, exactInput) !== exactOwner.count + 10) throw Error('exact warm');
}
Object.defineProperty(exactInput, 'value', {configurable: true, get() {
  events.push('exact-get:' + exactOwner.count);
  return 20;
}});
const exactBefore = exactOwner.count;
if (runExact(exactOwner, exactInput) !== exactBefore + 24 || exactOwner.count !== exactBefore + 1) {
  throw Error('callee prefix repeated before getter');
}
Object.defineProperty(exactInput, 'value', {get() {
  events.push('exact-throw:' + exactOwner.count);
  throw 'exact-error';
}});
try { runExact(exactOwner, exactInput); throw Error('missing callee error'); }
catch (error) { if (error !== 'exact-error') throw error; }
if (exactOwner.count !== exactBefore + 2) throw Error('callee prefix repeated before throw');
output.push(exactOwner.count);
exactPrototype.linkExact = function linkExactLeaf(head) {
  this.count = this.count + 1;
  this.link = null;
  while (head.link !== null) head = head.link;
  head.link = this;
  return head;
};
function runLinkExact(owner, head) {
  const result = owner.linkExact(head);
  owner.after = owner.after + 1;
  return result;
}
const linkOwner = Object.create(exactPrototype);
linkOwner.count = 0; linkOwner.link = null; linkOwner.after = 0;
const linkHead = {link: null};
for (let i = 0; i < 50; i++) {
  linkHead.link = null;
  if (runLinkExact(linkOwner, linkHead) !== linkHead || linkHead.link !== linkOwner) throw Error('link warm');
}
let storedLink;
const linkTail = {get link() {
  if (exactPrototype.linkExact.arguments[0] !== linkTail || exactPrototype.linkExact.caller !== runLinkExact) {
    throw Error('materialized reflection');
  }
  events.push('tail-get:' + linkOwner.count); return null;
},
  set link(value) { events.push('tail-set:' + linkOwner.count); storedLink = value; }};
linkHead.link = linkTail;
if (runLinkExact(linkOwner, linkHead) !== linkTail || storedLink !== linkOwner
    || linkOwner.count !== 51 || linkOwner.after !== 51) throw Error('link recovery');
output.push(linkOwner.count);
exactPrototype.strictExact = function (input) {
  'use strict';
  this.count = this.count + 1;
  input.value = this.count;
  return this.count;
};
function runStrictExact(owner, input) {
  const result = owner.strictExact(input);
  return result + 1;
}
const strictInput = {value: 0};
for (let i = 0; i < 50; i++) runStrictExact(exactOwner, strictInput);
Object.defineProperty(strictInput, 'value', {writable: false});
const strictBefore = exactOwner.count;
try { runStrictExact(exactOwner, strictInput); throw Error('missing strict failure'); }
catch (error) { if (error.name !== 'TypeError') throw error; }
if (exactOwner.count !== strictBefore + 1) throw Error('strict prefix replay');
output.push(exactOwner.count);
output.push(events.join(','));
console.log(JSON.stringify(output));
