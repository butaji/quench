'use strict';
// Language-primitive scoreboard. Each primitive prints `PRIM <name> <ms> <checksum>`;
// primitives.c runs the same workloads with the same counts and checksums.

const SEED = 12345;
const now = typeof performance !== 'undefined' ? () => performance.now() : () => Date.now();

// Exact double -> uint32 projection shared with the C side (floor, then mod 2^32).
function u32d(d) { return Math.floor(d) >>> 0; }
function lcg(x) { return (x * 1664525 + 1013904223) | 0; }

// ---- immutable setup tables (built untimed) ----
const PIECES = ['ab', 'cde', 'f', 'ghij', 'kl', 'mnopq', 'rs', 't'];
const WORDS = ['alpha', 'alphabet', 'alpine', 'beta', 'betamax', 'bet', 'gamma', 'gam',
  'delta', 'deltas', 'epsilon', 'eps', 'zeta', 'zed', 'eta', 'etas'];
const OPS = ['add', 'sub', 'mul', 'xor', 'shl', 'neg', 'nop', 'mov'];
const HEXES = ['deadbeef', 'cafe01', '0123456789abcdef', 'xyz', 'abcdefg', '12ab34cd', '',
  'ffff', 'a1b2c3d4e5', 'DEADBEEF', '00ff00ff00', 'beefcafe9', 'f00d', 'hello', '7', 'abc-def'];
const HEX_RE = /^[a-f0-9]+$/;
const LONG_LEN = 65536;
const LONG_STR = (function () {
  const parts = [];
  let x = SEED;
  for (let j = 0; j < LONG_LEN; j++) {
    x = lcg(x);
    parts.push(String.fromCharCode(97 + ((x >>> 16) % 26)));
  }
  return parts.join('');
})();
const SKEYS = [];
for (let j = 0; j < 512; j++) SKEYS.push('key' + j);
const DKEYS = [];
for (let j = 0; j < 64; j++) DKEYS.push('p' + j);
const THROWN = new Error('primitive');
THROWN.code = 7;

// ---- primitives ----
function intLoop(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    x = (x * 1664525 + 1013904223 + i) | 0;
    x = x ^ (x >>> 7);
  }
  return x;
}

function floatLoop(n) {
  let x = 0.5 + (SEED % 1000) * 1e-4;
  let s = 0;
  for (let i = 0; i < n; i++) {
    x = 3.9 * x * (1 - x);
    s += x;
  }
  return u32d(s * 1000);
}

function fib(k) { return k < 2 ? k : fib(k - 1) + fib(k - 2); }
function recursionFib(n) { return fib(n + (SEED & 3)); }

function stepDirect(x, i) { return ((x ^ i) * 31 + 7) | 0; }
function callDirect(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) x = stepDirect(x, i);
  return x;
}

function makeStepper(k) { return function (x, i) { return ((x ^ i) * k + 1) | 0; }; }
function callClosure(n) {
  const f = makeStepper((SEED & 15) + 3);
  let x = SEED;
  for (let i = 0; i < n; i++) x = f(x, i);
  return x;
}

function Acc(k, c) { this.k = k; this.c = c; }
Acc.prototype.step = function (x) { return ((x ^ this.k) * 31 + this.c) | 0; };
function callMethod(n) {
  const o = new Acc(SEED & 255, 7);
  let x = SEED;
  for (let i = 0; i < n; i++) x = (o.step(x) + i) | 0;
  return x;
}

class Sq { constructor(a) { this.a = a; } f(x) { return ((x ^ this.a) * 3 + 1) | 0; } }
class Ci { constructor(a) { this.b = 0; this.a = a; } f(x) { return (x + this.a * 5) | 0; } }
class Tr { constructor(a) { this.c = 1; this.d = 2; this.a = a; } f(x) { return ((x ^ (x >>> 3)) + this.a) | 0; } }
function callPolymorphic(n) {
  const objs = [new Sq(3), new Ci(5), new Tr(7)];
  let x = SEED;
  for (let i = 0; i < n; i++) x = objs[i % 3].f(x);
  return x;
}

function closureCreate(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    const k = i & 255;
    const f = (y) => ((y ^ k) * 5 + 1) | 0;
    x = f(x);
  }
  return x;
}

function P(a, b) { this.a = a; this.b = b; }
function objectAllocCtor(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    const p = new P(x & 1023, i);
    x = (x * 3 + p.a + p.b) | 0;
  }
  return x;
}

function objectAllocLiteral(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    const o = { x: x & 4095, y: i ^ 5 };
    x = (x * 5 + o.x - o.y) | 0;
  }
  return x;
}

function fieldReadWrite(n) {
  const o = { a: SEED, b: 1, c: 2 };
  for (let i = 0; i < n; i++) {
    o.a = (o.a + o.c) | 0;
    o.b = o.b ^ o.a;
    o.c = (o.c * 3 + i) | 0;
  }
  return o.a ^ o.b ^ o.c;
}

function fieldPolymorphic(n) {
  const v = SEED & 7;
  const shapes = [{ v: v + 1 }, { a: 0, v: v + 2 }, { a: 0, b: 0, v: v + 3 }, { a: 0, b: 0, c: 0, v: v + 4 }];
  let x = SEED;
  for (let i = 0; i < n; i++) x = (x * 3 + shapes[i & 3].v) | 0;
  return x;
}

function fieldAdd(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    const o = {};
    o.a = x & 255;
    o.b = i & 1023;
    o.c = (x >>> 8) & 255;
    o.d = 9;
    x = (x * 7 + o.a + o.b + o.c + o.d) | 0;
  }
  return x;
}

function arrayIndex(n) {
  const len = 4096;
  const a = [];
  for (let j = 0; j < len; j++) a.push((SEED + j * 7) | 0);
  let s = 0;
  for (let r = 0; r < n; r++) {
    for (let j = 0; j < len; j++) {
      s = (s + a[j]) | 0;
      a[j] = (a[j] * 5 + s) | 0;
    }
  }
  return s;
}

function arrayDouble(n) {
  const len = 4096;
  const a = [];
  for (let j = 0; j < len; j++) a.push((j % 17) * 0.37);
  let t = 0;
  for (let r = 0; r < n; r++) {
    for (let j = 0; j < len; j++) {
      const v = a[j] * 0.5 + a[(j + 1) & (len - 1)] * 0.25 + (j & 7) * 0.125;
      a[j] = v;
      t += v;
    }
  }
  return u32d(t * 16);
}

function arrayPushPop(n) {
  const a = [];
  let x = SEED;
  for (let r = 0; r < n; r++) {
    for (let j = 0; j < 1000; j++) a.push(x ^ j);
    while (a.length > 0) x = (x * 31 + a.pop()) | 0;
  }
  return x;
}

function arrayOfObjects(n) {
  let x = SEED;
  for (let r = 0; r < n; r++) {
    const arr = [];
    for (let j = 0; j < 1000; j++) arr.push({ v: (x + j) | 0, w: j });
    let s = 0;
    for (let j = 0; j < 1000; j++) {
      const o = arr[j];
      s = (s + (o.v ^ o.w)) | 0;
    }
    x = (x * 31 + s) | 0;
  }
  return x;
}

function typedF64(n) {
  const len = 4096;
  let a = new Float64Array(len);
  let b = new Float64Array(len);
  for (let j = 0; j < len; j++) { a[j] = 1 + (j & 15) * 0.0625; b[j] = a[j]; }
  a[0] = b[0] = SEED % 100;
  a[len - 1] = b[len - 1] = 0;
  for (let r = 0; r < n; r++) {
    for (let j = 1; j < len - 1; j++) b[j] = 0.25 * a[j - 1] + 0.5 * a[j] + 0.25 * a[j + 1];
    const t = a; a = b; b = t;
  }
  let s = 0;
  for (let j = 0; j < len; j++) s += a[j];
  return u32d(s * 1e6);
}

function typedU8(n) {
  const len = 65536;
  const u = new Uint8Array(len);
  for (let j = 0; j < len; j++) u[j] = (j * 7 + SEED) & 255;
  let x = SEED & 255;
  for (let r = 0; r < n; r++) {
    for (let j = 0; j < len; j++) {
      u[j] = u[j] + (u[(j + 1) & (len - 1)] ^ x);
      x = u[j];
    }
  }
  let s = 0;
  for (let j = 0; j < len; j++) s = (s + u[j]) | 0;
  return (s * 256 + x) | 0;
}

function typedI32(n) {
  const len = 16384;
  const a = new Int32Array(len);
  for (let j = 0; j < len; j++) a[j] = SEED * (j + 1);
  for (let r = 0; r < n; r++) {
    for (let j = 1; j < len; j++) a[j] = ((Math.imul(a[j], 0x01000193) ^ a[j - 1]) + r) | 0;
  }
  let s = 0;
  for (let j = 0; j < len; j++) s = (s + a[j]) | 0;
  return s;
}

function stringCharcode(n) {
  const s = LONG_STR;
  let h = SEED;
  for (let r = 0; r < n; r++) {
    for (let j = 0; j < s.length; j++) h = (h * 31 + s.charCodeAt(j)) | 0;
  }
  return h;
}

function stringConcat(n) {
  let h = SEED;
  for (let r = 0; r < n; r++) {
    let s = '';
    for (let j = 0; j < 1000; j++) s += PIECES[(h + j) & 7];
    for (let k = 0; k < s.length; k++) h = (h * 31 + s.charCodeAt(k)) | 0;
    h = (h + s.length) | 0;
  }
  return h;
}

function stringCompare(n) {
  let x = SEED;
  let h = 0;
  for (let i = 0; i < n; i++) {
    const a = WORDS[x & 15];
    const b = WORDS[(x >>> 4) & 15];
    h = (h * 31 + (a === b ? 1 : 0) + (a < b ? 2 : 0)) | 0;
    x = lcg(x);
  }
  return h;
}

function stringSwitch(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    switch (OPS[(x >>> 8) & 7]) {
      case 'add': x = (x + i) | 0; break;
      case 'sub': x = (x - 7) | 0; break;
      case 'mul': x = (x * 3) | 0; break;
      case 'xor': x = x ^ 0x5bd1; break;
      case 'shl': x = (x << 1) | 1; break;
      case 'neg': x = -x | 0; break;
      default: x = (x + 1) | 0;
    }
    x = lcg(x);
  }
  return x;
}

function intSwitch(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    switch ((x >>> 8) & 7) {
      case 0: x = (x + i) | 0; break;
      case 1: x = (x - 7) | 0; break;
      case 2: x = (x * 3) | 0; break;
      case 3: x = x ^ 0x5bd1; break;
      case 4: x = (x << 1) | 1; break;
      case 5: x = -x | 0; break;
      case 6: x = (x + 0x1234) | 0; break;
      case 7: x = x ^ (x >>> 5); break;
    }
    x = lcg(x);
  }
  return x;
}

// Shared get/update/delete pattern for both Map primitives.
function mapChurn(n, keyOf) {
  const m = new Map();
  let x = SEED;
  let s = 0;
  for (let i = 0; i < n; i++) {
    const k = keyOf(x);
    const v = m.get(k);
    if (v === undefined) m.set(k, i);
    else {
      s = (s + v) | 0;
      if ((v & 1) === 1) m.delete(k);
      else m.set(k, v + 1);
    }
    x = lcg(x);
  }
  return (s + m.size) | 0;
}
function mapIntKeys(n) { return mapChurn(n, (x) => (x >>> 8) & 8191); }
function mapStringKeys(n) { return mapChurn(n, (x) => SKEYS[(x >>> 8) & 511]); }

function objectDict(n) {
  const d = {};
  for (let j = 0; j < 64; j++) d[DKEYS[j]] = j;
  let x = SEED;
  for (let i = 0; i < n; i++) {
    const k = DKEYS[(x >>> 10) & 63];
    const v = (d[k] + i) | 0;
    d[k] = v;
    x = (x * 1664525 + 1013904223 + v) | 0;
  }
  let s = x;
  for (let j = 0; j < 64; j++) s = (s * 31 + d[DKEYS[j]]) | 0;
  return s;
}

function forOfArray(n) {
  const a = [];
  for (let j = 0; j < 4096; j++) a.push((j * 7919 + SEED) & 0xffff);
  let s = 0;
  for (let r = 0; r < n; r++) {
    for (const v of a) s = (s * 31 + v) | 0;
  }
  return s;
}

function arrayMapFilterReduce(n) {
  const base = [];
  for (let j = 0; j < 1000; j++) base.push((j * 37 + SEED) & 1023);
  let s = 0;
  let r = SEED & 255;
  for (let round = 0; round < n; round++) {
    const t = base.map((v) => (v * 3 + r) | 0)
      .filter((v) => (v & 3) !== 0)
      .reduce((acc, v) => (acc + v) | 0, 0);
    s = (s * 31 + t) | 0;
    r = s & 255;
  }
  return s;
}

function arraySortComparator(n) {
  const a = [];
  for (let j = 0; j < 1000; j++) a.push(0);
  let x = SEED;
  let h = 0;
  for (let r = 0; r < n; r++) {
    for (let j = 0; j < 1000; j++) { x = lcg(x); a[j] = x >>> 12; }
    a.sort((p, q) => p - q);
    for (let j = 0; j < 1000; j++) h = (h * 31 + a[j]) | 0;
  }
  return h;
}

function mathFunctions(n) {
  let x = SEED;
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const t = Math.sqrt((x >>> 0) + 0.5);
    const f = Math.floor(t * 3.7);
    const a = Math.abs(x >> 16);
    const m = Math.min(Math.max(f, a), 200000);
    x = (Math.imul(x, 1103515245) + m + 12345) | 0;
    acc += t;
  }
  return x ^ u32d(acc);
}

function tryCatchNoThrow(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    try {
      x = (x * 31 + i) | 0;
      x = x ^ (x >>> 9);
      if (x === 0x7ffffffe) throw new Error('unreachable');
    } catch (e) {
      x = 1;
    }
  }
  return x;
}

function maybeThrow(x, i) {
  if ((i & 15) === 0) throw THROWN;
  return (x * 31 + i) | 0;
}
function tryCatchThrow(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    try {
      x = maybeThrow(x, i);
    } catch (e) {
      x = (x + e.code) | 0;
    }
  }
  return x;
}

class Base { constructor(a) { this.a = a; } }
class Derived extends Base { constructor(a) { super(a); this.b = 1; } }
const MIXED = [3, 'hello', new Base(5), 2.5, true, new Derived(6), null, 'ab'];
function instanceofTypeof(n) {
  let s = SEED;
  for (let i = 0; i < n; i++) {
    const v = MIXED[(i ^ (s >>> 3)) & 7];
    let c;
    if (typeof v === 'number') c = v | 0;
    else if (typeof v === 'string') c = v.length;
    else if (typeof v === 'boolean') c = v ? 11 : 13;
    else if (v instanceof Derived) c = v.a * 2;
    else if (v instanceof Base) c = v.a;
    else c = 17;
    s = (s * 3 + c) | 0;
  }
  return s;
}

class Box {
  constructor(v) { this._v = v; }
  get v() { return this._v; }
  set v(n) { this._v = n | 0; }
}
function classGetterSetter(n) {
  const b = new Box(SEED);
  for (let i = 0; i < n; i++) b.v = b.v * 3 + i;
  return b.v;
}

class Priv {
  #x;
  constructor(x) { this.#x = x; }
  run(n) {
    for (let i = 0; i < n; i++) this.#x = (this.#x * 5 + i) | 0;
    return this.#x;
  }
}
function classPrivateField(n) { return new Priv(SEED).run(n); }

function* counter(n, seed) {
  let x = seed;
  for (let i = 0; i < n; i++) {
    x = lcg(x);
    yield x >>> 16;
  }
}
function generatorIterate(n) {
  let s = 0;
  for (const v of counter(n, SEED)) s = (s * 31 + v) | 0;
  return s;
}

async function addAsync(x, i) { return (x * 31 + i) | 0; }
async function promiseChain(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) x = await addAsync(x, i);
  return x;
}

function sumRest(...xs) {
  let s = 0;
  for (let k = 0; k < xs.length; k++) s = (s * 3 + xs[k]) | 0;
  return s;
}
function restSpreadArgs(n) {
  const args = [SEED, 0, 7, 11];
  let x = SEED;
  for (let i = 0; i < n; i++) {
    args[0] = x;
    args[1] = i;
    x = sumRest(...args);
  }
  return x;
}

function sumArgs() {
  let s = 0;
  for (let k = 0; k < arguments.length; k++) s = (s * 3 + arguments[k]) | 0;
  return s;
}
function argumentsObject(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) x = sumArgs(x, i, 13, 5, 3);
  return x;
}

function bigintArith(n) {
  const M = 4294967291n;
  const A = 2654435761n;
  let x = BigInt(SEED);
  let c = 0n;
  for (let i = 0; i < n; i++) {
    x = (x * A + c) % M;
    c += 1n;
  }
  return Number(x);
}

function regexpTest(n) {
  let h = SEED;
  for (let i = 0; i < n; i++) {
    const k = (i ^ (h >>> 5)) & 15;
    h = (h * 31 + (HEX_RE.test(HEXES[k]) ? k + 1 : 0)) | 0;
  }
  return h;
}

function jsonRoundtrip(n) {
  let x = SEED;
  for (let i = 0; i < n; i++) {
    const o = { id: i, kind: 'pt', x: x & 0xffff, y: x >>> 16, ok: (x & 1) === 1, tag: 'q' };
    const s = JSON.stringify(o);
    const p = JSON.parse(s);
    x = (x * 31 + p.id + p.x + p.y + (p.ok ? 1 : 0) + s.length + p.kind.length + p.tag.length) | 0;
  }
  return x;
}

// ---- runner ----
const PRIMITIVES = [
  { name: 'int-loop', n: 50000000, run: intLoop },
  { name: 'float-loop', n: 20000000, run: floatLoop },
  { name: 'recursion-fib', n: 34, run: recursionFib },
  { name: 'call-direct', n: 50000000, run: callDirect },
  { name: 'call-closure', n: 50000000, run: callClosure },
  { name: 'call-method', n: 50000000, run: callMethod },
  { name: 'call-polymorphic', n: 30000000, run: callPolymorphic },
  { name: 'closure-create', n: 10000000, run: closureCreate },
  { name: 'object-alloc-ctor', n: 10000000, run: objectAllocCtor },
  { name: 'object-alloc-literal', n: 10000000, run: objectAllocLiteral },
  { name: 'field-read-write', n: 50000000, run: fieldReadWrite },
  { name: 'field-polymorphic', n: 50000000, run: fieldPolymorphic },
  { name: 'field-add', n: 10000000, run: fieldAdd },
  { name: 'array-index', n: 20000, run: arrayIndex },
  { name: 'array-double', n: 5000, run: arrayDouble },
  { name: 'array-push-pop', n: 20000, run: arrayPushPop },
  { name: 'array-of-objects', n: 5000, run: arrayOfObjects },
  { name: 'typed-f64', n: 20000, run: typedF64 },
  { name: 'typed-u8', n: 500, run: typedU8 },
  { name: 'typed-i32', n: 2000, run: typedI32 },
  { name: 'string-charcode', n: 500, run: stringCharcode },
  { name: 'string-concat', n: 8000, run: stringConcat },
  { name: 'string-compare', n: 20000000, run: stringCompare },
  { name: 'string-switch', n: 10000000, run: stringSwitch },
  { name: 'int-switch', n: 30000000, run: intSwitch },
  { name: 'map-int-keys', n: 10000000, run: mapIntKeys },
  { name: 'map-string-keys', n: 5000000, run: mapStringKeys },
  { name: 'object-dict', n: 10000000, run: objectDict },
  { name: 'for-of-array', n: 10000, run: forOfArray },
  { name: 'array-map-filter-reduce', n: 30000, run: arrayMapFilterReduce },
  { name: 'array-sort-comparator', n: 2000, run: arraySortComparator },
  { name: 'math-functions', n: 8000000, run: mathFunctions },
  { name: 'try-catch-nothrow', n: 50000000, run: tryCatchNoThrow },
  { name: 'try-catch-throw', n: 30000000, run: tryCatchThrow },
  { name: 'instanceof-typeof', n: 20000000, run: instanceofTypeof },
  { name: 'class-getter-setter', n: 50000000, run: classGetterSetter },
  { name: 'class-private-field', n: 50000000, run: classPrivateField },
  { name: 'generator-iterate', n: 20000000, run: generatorIterate },
  { name: 'promise-chain', n: 10000000, run: promiseChain, async: true },
  { name: 'rest-spread-args', n: 20000000, run: restSpreadArgs },
  { name: 'arguments-object', n: 20000000, run: argumentsObject },
  { name: 'bigint-arith', n: 10000000, run: bigintArith },
  { name: 'regexp-test', n: 8000000, run: regexpTest },
  { name: 'json-roundtrip', n: 500000, run: jsonRoundtrip },
];

async function main() {
  for (const p of PRIMITIVES) {
    const t0 = now();
    const sum = p.async ? await p.run(p.n) : p.run(p.n);
    const ms = now() - t0;
    console.log('PRIM ' + p.name + ' ' + ms.toFixed(3) + ' ' + (sum >>> 0));
  }
}
main();
