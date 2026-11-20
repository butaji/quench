function pair(o) { return [o.a, o.b]; }
function sum(o) { return o.a + o.b; }
function reversed(o) { return o.b + o.a; }
function same(o) { return o.a + o.a; }
function overwrite(o) { o = o.a; return o.b; }
function warm(o) {
  let total = 0;
  for (let i = 0; i < 300; i++) total += sum(o) + reversed(o) + same(o);
  return total;
}
const small = {a: 1, b: 2};
const arena = {a: 3, b: 4, c: 5, d: 6};
console.log(warm(small), warm(arena));
console.log(JSON.stringify(pair(small)), JSON.stringify(pair(arena)));
small.a = 7; small.b = 8;
console.log(sum(small), same(small));
small.a = 1.25; small.b = 'wide';
console.log(JSON.stringify(pair(small)));
Object.preventExtensions(arena);
console.log(warm(arena));
const prototype = {marker: 1};
Object.setPrototypeOf(small, prototype);
console.log(JSON.stringify(pair(small)));
delete small.b;
prototype.b = 9;
console.log(JSON.stringify(pair(small)));
let events = [];
Object.defineProperty(small, 'a', {configurable: true, get() { events.push('a'); this.b = 11; return 2; }});
console.log(JSON.stringify(pair(small)), JSON.stringify(events));
const proxied = new Proxy({a: 5, b: 6}, {get(o, key) { events.push(key); return o[key]; }});
events = [];
console.log(sum(proxied), JSON.stringify(events));
console.log(overwrite({a: {b: 17}, b: 99}));
for (const value of [null, undefined]) {
  try { pair(value); } catch (e) { console.log(e.name); }
}
const stable = {a: 13, b: 14};
console.log(warm(stable));
for (let round = 0; round < 12; round++) {
  const objects = [];
  for (let i = 0; i < 4000; i++) objects.push({['key' + round + '_' + i]: {a: i, b: i + 1}});
  console.log(sum(stable), pair(stable).join(','));
}
console.log(JSON.stringify(pair({a: 0, b: 0})), JSON.stringify(pair({a: null, b: undefined})));

function thisSum() { return this.a + this.b; }
function gather(a, b, c) { return [a, b, c]; }
function triple(o) { return gather(o.a, o.b, o.c); }
const receiver = {a: 19, b: 20, c: 21};
let thisTotal = 0;
for (let i = 0; i < 300; i++) {
  thisTotal += thisSum.call(receiver);
  triple(receiver);
}
receiver.b = 25;
console.log(thisTotal, thisSum.call(receiver), JSON.stringify(triple(receiver)));
Object.defineProperty(receiver, 'b', {get() { return 30; }});
console.log(thisSum.call(receiver), JSON.stringify(triple(receiver)));

function classify(o) {
  if (o.a === undefined) return 'undefined';
  if (o.a === null) return 'null';
  if (o.a != null) return 'value';
  return 'other';
}
function thisClassify() { if (this.a != null) return 7; return 9; }
const subject = {a: 0};
for (let i = 0; i < 500; i++) classify(subject);
for (const value of [undefined, null, 0, false, {}, '', Symbol('x'), 1n]) {
  subject.a = value;
  console.log(classify(subject), thisClassify.call(subject));
}
let reads = 0;
const changing = {get a() { reads++; return reads === 1 ? 0 : null; }};
console.log(classify(changing), reads);
const strict = o => { if (o.a !== undefined) return true; return false; };
for (let i = 0; i < 500; i++) strict(subject);
console.log(strict({a: undefined}), strict({a: null}));

function liveAfter(o) { const v = o.a; if (v === null) return v; return [v, v]; }
for (let i = 0; i < 500; i++) liveAfter(subject);
console.log(JSON.stringify(liveAfter({a: null})), JSON.stringify(liveAfter({a: 'retained'})));

console.log(classify(subject));
function cycle(o) {
  let n = 0;
  do { n++; if (n === 5) o.a = null; } while (o.a !== null);
  return n;
}
console.log(cycle({a: 1}));

function forward(o) { const v = o.a; return [v, v, o]; }
function retain(o) { let v = o.a; let w = v; if (w) return [v, w]; return [w, v]; }
const moved = {a: 23};
for (let i = 0; i < 500; i++) { forward(moved); retain(moved); }
console.log(JSON.stringify(forward(moved)), JSON.stringify(retain(moved)));

const rooted = {a: 31, b: 37, c: 41};
function callback() { return sum(rooted); }
for (let round = 0; round < 40; round++) {
  const values = Array.from({length: 40}, (_, i) => ({a: i, b: i + 1}));
  const mapped = values.map(o => sum(o) + callback());
  JSON.stringify({get value() { return callback(); }});
  if (round === 39) console.log(mapped[0], mapped[39], callback());
}
Promise.resolve().then(() => console.log('resumed', callback()));

const wasmModule = new WebAssembly.Module(new Uint8Array([
  0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,
  3,2,1,0,7,7,1,3,97,100,100,0,0,10,9,1,7,0,32,0,32,1,106,11
]));
const wasmAdd = new WebAssembly.Instance(wasmModule).exports.add;
let wasmTotal = 0;
for (let i = 0; i < 5000; i++) wasmTotal += wasmAdd(i, 1);
console.log('wasm', wasmTotal, callback());
