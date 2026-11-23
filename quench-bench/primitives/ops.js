// Primitive-operation micro-benchmarks: one tight loop per operation, fixed work, result used.
// Each op(n) returns a value that the runner folds into a checksum so nothing is dead code.
"use strict";
const ops = {};
const S = "the quick brown fox jumps over the lazy dog 0123456789";
ops.empty = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = i; } return a; };
ops.int_add = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + i) | 0; } return a; };
ops.int_add_noor = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = a + (i & 7); } return a; };
ops.int_sub = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a - i) | 0; } return a; };
ops.int_mul = function (n) { let a = 1; for (let i = 0; i < n; i++) { a = (a * 3 + i) | 0; } return a; };
ops.int_mul_ovf = function (n) { let a = 1; for (let i = 0; i < n; i++) { a = (a * 65537) % 1000003; } return a; };
ops.int_div = function (n) { let a = 0; for (let i = 1; i < n; i++) { a = (a + ((1000003 / i) | 0)) | 0; } return a; };
ops.int_mod = function (n) { let a = 0; for (let i = 1; i < n; i++) { a = (a + (i % 7)) | 0; } return a; };
ops.int_mod_var = function (n) { let a = 0, m = 13; for (let i = 1; i < n; i++) { a = (a + (i % m)) | 0; } return a; };
ops.shl_shr = function (n) { let a = 1; for (let i = 0; i < n; i++) { a = ((a << 3) ^ (a >> 2) ^ i) | 0; } return a; };
ops.ushr = function (n) { let a = -1; for (let i = 0; i < n; i++) { a = (a >>> 1) + i | 0; } return a; };
ops.bitwise = function (n) { let a = 0x12345; for (let i = 0; i < n; i++) { a = ((a & 0xff00ff) | (i ^ 0x55)) & 0xfffffff; } return a; };
ops.to_uint32 = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + ((i * 2654435761) >>> 0)) % 4294967296; } return a; };
ops.dbl_add = function (n) { let a = 0.5; for (let i = 0; i < n; i++) { a = a + 0.25; } return a; };
ops.dbl_mul = function (n) { let a = 1.0000001; for (let i = 0; i < n; i++) { a = a * 1.0000001; } return a; };
ops.dbl_div = function (n) { let a = 0; for (let i = 1; i < n; i++) { a = a + 1 / i; } return a; };
ops.mixed_int_dbl = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = a + i * 0.5; } return a; };
ops.cmp_int = function (n) { let c = 0; for (let i = 0; i < n; i++) { if (i < 1000 || (i & 3) > 1) c++; } return c; };
ops.cmp_dbl = function (n) { let c = 0, x = 0.5; for (let i = 0; i < n; i++) { x += 0.001; if (x < 100.5) c++; } return c; };
ops.eq_int = function (n) { let c = 0; for (let i = 0; i < n; i++) { if ((i & 7) === 3) c++; } return c; };
ops.eq_str = function (n) { let c = 0, a = "abc", b = "abd", d = "abc"; for (let i = 0; i < n; i++) { if (a === d) c++; if (a === b) c--; } return c; };
ops.eq_undef_null = function (n) { let c = 0, u; for (let i = 0; i < n; i++) { if (u === undefined) c++; if (u == null) c++; } return c; };
ops.truthy_int = function (n) { let c = 0; for (let i = 0; i < n; i++) { if (i & 3) c++; } return c; };
ops.truthy_obj = function (n) { let c = 0, o = { a: 1 }; for (let i = 0; i < n; i++) { if (o) c++; } return c; };
ops.truthy_str = function (n) { let c = 0, s = "x"; for (let i = 0; i < n; i++) { if (s) c++; } return c; };
ops.not_op = function (n) { let c = 0; for (let i = 0; i < n; i++) { if (!(i & 1)) c++; } return c; };
ops.neg_op = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (-a + i) | 0; } return a; };
ops.typeof_op = function (n) { let c = 0, x = 5; for (let i = 0; i < n; i++) { if (typeof x === "number") c++; } return c; };
ops.inc_local = function (n) { let a = 0, b = 0; for (let i = 0; i < n; i++) { a++; b--; } return a + b + 1; };
ops.math_floor = function (n) { let a = 0; for (let i = 0; i < n; i++) { a += Math.floor(i * 0.37); } return a; };
ops.math_sqrt = function (n) { let a = 0; for (let i = 0; i < n; i++) { a += Math.sqrt(i); } return a; };
ops.math_abs = function (n) { let a = 0; for (let i = 0; i < n; i++) { a += Math.abs(i - 500); } return a; };
ops.math_minmax = function (n) { let a = 0; for (let i = 0; i < n; i++) { a += Math.min(i, 777) + Math.max(i, 5); } return a; };
ops.math_imul = function (n) { let a = 1; for (let i = 0; i < n; i++) { a = Math.imul(a, 31) + i | 0; } return a; };
ops.math_round = function (n) { let a = 0; for (let i = 0; i < n; i++) { a += Math.round(i * 0.37); } return a; };
ops.num_to_str = function (n) { let a = 0; for (let i = 0; i < n; i++) { a += ("" + i).length; } return a; };
ops.str_to_num = function (n) { let a = 0, s = "12345"; for (let i = 0; i < n; i++) { a = (a + +s) | 0; } return a; };
ops.parse_int = function (n) { let a = 0, s = "12345"; for (let i = 0; i < n; i++) { a = (a + parseInt(s, 10)) | 0; } return a; };
ops.str_length = function (n) { let a = 0, s = S; for (let i = 0; i < n; i++) { a += s.length; } return a; };
ops.str_charcodeat = function (n) { let a = 0, s = S, l = s.length; for (let i = 0; i < n; i++) { a = (a + s.charCodeAt(i % l)) | 0; } return a; };
ops.str_index = function (n) { let a = 0, s = S, l = s.length; for (let i = 0; i < n; i++) { if (s[i % l] === "o") a++; } return a; };
ops.str_concat = function (n) { let a = 0; for (let i = 0; i < n; i++) { const t = "ab" + (i & 15); a += t.length; } return a; };
ops.str_concat_acc = function (n) { let s = ""; for (let i = 0; i < n; i++) { s += "x"; } return s.length; };
ops.str_compare = function (n) { let c = 0, a = "apple", b = "apricot"; for (let i = 0; i < n; i++) { if (a < b) c++; } return c; };
ops.arr_load_int = function (n) { const arr = new Array(256); for (let i = 0; i < 256; i++) arr[i] = i; let a = 0; for (let i = 0; i < n; i++) { a = (a + arr[i & 255]) | 0; } return a; };
ops.arr_store_int = function (n) { const arr = new Array(256); for (let i = 0; i < 256; i++) arr[i] = 0; for (let i = 0; i < n; i++) { arr[i & 255] = i; } return arr[7]; };
ops.arr_load_dbl = function (n) { const arr = new Array(256); for (let i = 0; i < 256; i++) arr[i] = i + 0.5; let a = 0; for (let i = 0; i < n; i++) { a += arr[i & 255]; } return a; };
ops.arr_store_dbl = function (n) { const arr = new Array(256); for (let i = 0; i < 256; i++) arr[i] = 0.5; for (let i = 0; i < n; i++) { arr[i & 255] = i * 0.5; } return arr[7]; };
ops.arr_load_obj = function (n) { const arr = new Array(256); for (let i = 0; i < 256; i++) arr[i] = { v: i }; let a = 0; for (let i = 0; i < n; i++) { a = (a + arr[i & 255].v) | 0; } return a; };
ops.arr_store_obj = function (n) { const arr = new Array(256); const o = { v: 1 }; for (let i = 0; i < 256; i++) arr[i] = o; for (let i = 0; i < n; i++) { arr[i & 255] = o; } return arr.length; };
ops.arr_length = function (n) { const arr = [1, 2, 3, 4, 5]; let a = 0; for (let i = 0; i < n; i++) { a += arr.length; } return a; };
ops.arr_push = function (n) { let arr = []; for (let i = 0; i < n; i++) { arr.push(i); } return arr.length; };
ops.typed_load = function (n) { const t = new Int32Array(256); for (let i = 0; i < 256; i++) t[i] = i; let a = 0; for (let i = 0; i < n; i++) { a = (a + t[i & 255]) | 0; } return a; };
ops.typed_store_f64 = function (n) { const t = new Float64Array(256); for (let i = 0; i < n; i++) { t[i & 255] = i * 0.5; } return t[7]; };
ops.typed_store_u8c = function (n) { const t = new Uint8ClampedArray(256); for (let i = 0; i < n; i++) { t[i & 255] = (i & 511) * 0.75 - 64; } return t[7] + t[200]; };
function addOne(x) { return (x + 1) | 0; }
// The object arrives as a parameter, so the front end cannot fold `o.y` to a constant.
function fieldRead(n, o) { let a = 0; for (let i = 0; i < n; i++) { a = (a + o.y) | 0; } return a; }
ops.field_read = function (n) { return fieldRead(n, { x: 3, y: 4 }); };
ops.field_store = function (n) { const o = { x: 0, y: 0 }; for (let i = 0; i < n; i++) { o.y = i; } return o.y; };
ops.call_direct = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + addOne(i)) | 0; } return a; };
// A rest parameter read by index and length (`(...xs) => xs[0] + xs[1]`), as variadic
// helpers and wrappers write it: the values passed are read in place, no array is made.
const sumRest = (...xs) => (xs.length > 1 ? (xs[0] + xs[1]) | 0 : 0);
ops.call_rest = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + sumRest(i, 1)) | 0; } return a; };
// A call spreading an array (`f(...xs)`), the variadic call of a list held as an array.
ops.call_spread = function (n) { const xs = [0, 1]; let a = 0; for (let i = 0; i < n; i++) { xs[0] = i; a = (a + sumRest(...xs)) | 0; } return a; };
// A rest parameter forwarded whole (`(...xs) => g(...xs)`), the wrapper and decorator shape.
const forwardRest = (...xs) => sumRest(...xs);
ops.call_rest_forward = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + forwardRest(i, 1)) | 0; } return a; };
ops.call_closure = function (n) { const k = 5, f = (x) => (x + k) | 0; let a = 0; for (let i = 0; i < n; i++) { a = (a + f(i)) | 0; } return a; };
ops.call_method = function (n) { const o = { k: 1, m(x) { return (x + this.k) | 0; } }; let a = 0; for (let i = 0; i < n; i++) { a = (a + o.m(i)) | 0; } return a; };
// RegExp.prototype.test of typical patterns over one text (the regexp kit's subject); the C
// twins in ops.c are the matcher a C programmer writes for each pattern.
const RS = "The quick brown fox jumps over the lazy dog; contact: john.doe+tag@example.com, price $1234.56, date 2026-11-18, running jumping";
function rxLoop(n, re) { let c = 0; for (let i = 0; i < n; i++) { if (re.test(RS)) c++; } return c; }
ops.rx_lit = function (n) { return rxLoop(n, /lazy/); };
ops.rx_digits = function (n) { return rxLoop(n, /\d+/); };
ops.rx_word2 = function (n) { return rxLoop(n, /(\w+)\s(\w+)/); };
ops.rx_email = function (n) { return rxLoop(n, /[\w.+-]+@[\w-]+\.[\w.]+/); };
ops.rx_dotstar = function (n) { return rxLoop(n, /q.*z/); };
ops.rx_ahead = function (n) { return rxLoop(n, /\w+(?=@)/); };
// Kernels: one inner iteration of a benchmark-shaped loop (n-body pair step, struct array,
// polymorphic dispatch, JSON round trip, regexp test, string building), budgeted per
// iteration against C twins as the primitives are; a kernel's split names the primitive
// handlers it is made of.
function Body(x, y, z, vx, vy, vz, mass) { this.x = x; this.y = y; this.z = z; this.vx = vx; this.vy = vy; this.vz = vz; this.mass = mass; }
ops.k_nbody_pair = function (n) {
  const a = new Body(0, 0, 0, 0, 0, 0, 39.47), b = new Body(4.84, -1.16, -0.1, 0.6, 2.81, -0.02, 0.037);
  for (let i = 0; i < n; i++) {
    const dx = a.x - b.x, dy = a.y - b.y, dz = a.z - b.z;
    const d2 = dx * dx + dy * dy + dz * dz;
    const mag = 0.01 / (d2 * Math.sqrt(d2));
    const bm = b.mass * mag, am = a.mass * mag;
    a.vx -= dx * bm; a.vy -= dy * bm; a.vz -= dz * bm;
    b.vx += dx * am; b.vy += dy * am; b.vz += dz * am;
  }
  return a.vx + b.vz;
};
// Allocation: an object literal, a function constructor's instance and a class instance per
// iteration, each read back; the C twins allocate the same struct on the heap (malloc + free).
// A one-field object literal per iteration (`{ v: i }`), consumed locally.
ops.alloc_literal1 = function (n) { let s = 0; for (let i = 0; i < n; i++) { const p = { v: i }; s = (s + p.v) | 0; } return s; };
ops.alloc_literal = function (n) { let s = 0; for (let i = 0; i < n; i++) { const p = { x: i, y: 1 }; s = (s + p.x + p.y) | 0; } return s; };
function Pair(x, y) { this.x = x; this.y = y; }
ops.alloc_ctor = function (n) { let s = 0; for (let i = 0; i < n; i++) { const p = new Pair(i, 1); s = (s + p.x + p.y) | 0; } return s; };
class Triple { constructor(x, y, z) { this.x = x; this.y = y; this.z = z; } }
ops.alloc_class = function (n) { let s = 0; for (let i = 0; i < n; i++) { const p = new Triple(i, 1, 2); s = (s + p.x + p.z) | 0; } return s; };
ops.k_struct_sum = function (n) { const pts = []; for (let i = 0; i < 64; i++) pts.push({ x: i, y: i + 1 }); let s = 0; for (let i = 0; i < n; i++) { const p = pts[i & 63]; s = (s + p.x * p.y) | 0; } return s; };
class KernelTaskA { constructor(k) { this.k = k; } run(x) { return (x + this.k) | 0; } }
class KernelTaskB { constructor(k) { this.k = k; this.j = 1; } run(x) { return (x ^ this.k) | 0; } }
ops.k_dispatch = function (n) { const tasks = [new KernelTaskA(3), new KernelTaskB(5), new KernelTaskA(7), new KernelTaskB(9)]; let a = 0; for (let i = 0; i < n; i++) { a = tasks[i & 3].run(a + i); } return a; };
ops.k_json = function (n) { const o = { id: 0, name: "ab", ok: true }; let a = 0; for (let i = 0; i < n; i++) { o.id = i & 1023; const t = JSON.stringify(o); a = (a + JSON.parse(t).id + t.length) | 0; } return a; };
ops.k_regexp = function (n) { const r = /o+x/, words = ["foox", "bar", "ox", "zoooox"]; let a = 0; for (let i = 0; i < n; i++) { if (r.test(words[i & 3])) a++; } return a; };
ops.k_str_build = function (n) { let a = 0, s = ""; for (let i = 0; i < n; i++) { s += "k" + (i & 7); if ((i & 63) === 63) { a += s.length; s = ""; } } return a + s.length; };
// Builtins that run no user code: one call's fixed cost (Date.now) and a dense search's cost
// per element (includes / indexOf over 64 int32, about 40 elements read per call).
ops.date_now = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + (Date.now() & 1)) | 0; } return a > -1 ? 1 : 0; };
ops.arr_includes = function (n) { const arr = []; for (let j = 0; j < 64; j++) arr.push(j * 2); let a = 0; for (let i = 0; i < n; i++) { if (arr.includes(i & 127)) a++; } return a; };
ops.arr_indexof = function (n) { const arr = []; for (let j = 0; j < 64; j++) arr.push(j * 2); let a = 0; for (let i = 0; i < n; i++) { a = (a + arr.indexOf(i & 127)) | 0; } return a; };
// The twenty C-parity kernels (TOD-1807, scoreboard in crates/quench-tier-corpus/ir_budget.txt):
// each isolates one mechanism, with a C twin in ops.c of the same work per iteration. Inner
// loops run over 16 elements unless noted, so an iteration is a fixed amount of work.
function kvFib(n) { return n < 2 ? n : (kvFib(n - 1) + kvFib(n - 2)) | 0; }
function kvAdd(a, b) { return (a + b) | 0; }
function KvNode(val, next) { this.val = val; this.next = next; }
class KvRect { constructor(w, h) { this.w = w; this.h = h; } area() { return this.w * this.h; } }
function kvGcd(a, b) { while (b !== 0) { const t = a % b; a = b; b = t; } return a; }
ops.k01_sum_or = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i) | 0; } return s; };
ops.k02_fib = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + kvFib(10)) | 0; } return s; };
ops.k03_call_add = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = kvAdd(s, i); } return s; };
function kvDot(n, p) { let s = 0; for (let i = 0; i < n; i++) { s += p.x * p.x + p.y * p.y; } return s; }
ops.k04_dot = function (n) { return kvDot(n, { x: 1.5, y: 2.25 }); };
function kvFieldAdd(n, p, dx) { for (let i = 0; i < n; i++) { p.x += dx; } return p.x; }
ops.k05_field_add = function (n) { return kvFieldAdd(n, { x: 0.5, y: 0 }, 0.25); };
ops.k06_list = function (n) {
  let head = null; for (let j = 15; j >= 0; j--) head = new KvNode(j, head);
  let s = 0; for (let i = 0; i < n; i++) { let p = head; while (p) { s = (s + p.val) | 0; p = p.next; } } return s;
};
ops.k07_area = function (n) { const r = new KvRect(3, 4); let s = 0; for (let i = 0; i < n; i++) { s = (s + r.area()) | 0; } return s; };
function kvEscape(n, a, b) { let s = 0; for (let i = 0; i < n; i++) { const v = { x: a.x + b.x, y: a.y + b.y }; s += v.x * v.y; } return s; }
ops.k08_escape = function (n) { return kvEscape(n, { x: 1.5, y: 2 }, { x: 0.25, y: 3 }); };
ops.k09_reduce = function (n) {
  const arr = []; for (let j = 0; j < 16; j++) arr.push(j);
  let s = 0; for (let i = 0; i < n; i++) { s = (s + arr.reduce((t, v) => t + v, 0)) | 0; } return s;
};
ops.k10_f64_sum = function (n) {
  const a = new Float64Array(16); for (let j = 0; j < 16; j++) a[j] = j + 0.5;
  let s = 0; for (let i = 0; i < n; i++) { for (let j = 0; j < a.length; j++) s += a[j]; } return s;
};
ops.k11_saxpy = function (n) {
  const x = new Float64Array(16), y = new Float64Array(16); for (let j = 0; j < 16; j++) x[j] = j;
  const k = 0.5; for (let i = 0; i < n; i++) { for (let j = 0; j < y.length; j++) y[j] += k * x[j]; } return y[15];
};
ops.k12_copy = function (n) {
  const src = new Int32Array(16), dst = new Int32Array(16); for (let j = 0; j < 16; j++) src[j] = j;
  let s = 0; for (let i = 0; i < n; i++) { src[i & 15] = i; for (let j = 0; j < src.length; j++) dst[j] = src[j]; s = (s + dst[i & 15]) | 0; } return s;
};
ops.k13_matmul = function (n) {
  const N = 4, a = new Float64Array(16), b = new Float64Array(16), c = new Float64Array(16);
  for (let j = 0; j < 16; j++) { a[j] = j * 0.25; b[j] = 1 - j * 0.125; }
  for (let r = 0; r < n; r++) {
    for (let i = 0; i < N; i++) for (let j = 0; j < N; j++) { let t = 0; for (let k = 0; k < N; k++) t += a[i * N + k] * b[k * N + j]; c[i * N + j] = t; }
    a[r & 15] = (r & 7) * 0.125;
  }
  return c[5];
};
ops.k14_bsearch = function (n) {
  const a = new Int32Array(64); for (let j = 0; j < 64; j++) a[j] = j * 2;
  let s = 0; for (let i = 0; i < n; i++) { const key = (i * 7) & 127; let lo = 0, hi = a.length; while (lo < hi) { const mid = (lo + hi) >>> 1; if (a[mid] < key) lo = mid + 1; else hi = mid; } s = (s + lo) | 0; } return s;
};
ops.k15_isort = function (n) {
  const a = new Int32Array(8); let s = 0;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < 8; j++) a[j] = ((j * 5 + i) & 7) * 3;
    for (let j = 1; j < a.length; j++) { const v = a[j]; let k = j - 1; while (k >= 0 && a[k] > v) { a[k + 1] = a[k]; k--; } a[k + 1] = v; }
    s = (s + a[i & 7]) | 0;
  }
  return s;
};
ops.k16_sieve = function (n) {
  const f = new Uint8Array(64); let s = 0;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < 64; j++) f[j] = 0;
    for (let p = 2; p * p < 64; p++) if (f[p] === 0) for (let m = p * p; m < 64; m += p) f[m] = 1;
    s = (s + f[i & 63]) | 0;
  }
  return s;
};
ops.k17_hash = function (n) {
  const str = "abcdefghijklmnop"; let h = 0;
  for (let i = 0; i < n; i++) { for (let j = 0; j < str.length; j++) h = (Math.imul(h, 31) + str.charCodeAt(j)) | 0; } return h;
};
ops.k18_math = function (n) {
  let s = 0;
  for (let i = 0; i < n; i++) { const x = i * 0.5 + 0.25; s += Math.sqrt(x) + Math.abs(x - 100) + Math.floor(x) + Math.clz32(i) + (i >>> 3); } return s;
};
ops.k19_gcd = function (n) { let s = 0; for (let i = 0; i < n; i++) { const a = (i & 1023) + 1000; s = (s + kvGcd(a, 360) + ((a / 7) | 0)) | 0; } return s; };
ops.k20_switch = function (n) {
  const code = new Uint8Array(16); for (let j = 0; j < 16; j++) code[j] = (j * 7 + 3) & 3;
  let acc = 1;
  for (let i = 0; i < n; i++) {
    for (let pc = 0; pc < code.length; pc++) {
      switch (code[pc]) { case 0: acc = (acc + 1) | 0; break; case 1: acc = acc ^ 0x5a; break; case 2: acc = (acc * 3) | 0; break; case 3: acc = acc >> 1; break; }
    }
  }
  return acc;
};
// Kernels 4, 5, 6 and 8 of the C-parity table: a shape proven once with unboxed double fields,
// an in-place double field store in a loop, a pointer chase whose `next` is the same shape or
// null, and a literal consumed locally (escape analysis and scalar replacement).
ops.k_dist2 = function (n) { const pts = []; for (let i = 0; i < 64; i++) pts.push({ x: i + 0.5, y: i + 0.25 }); let s = 0; for (let i = 0; i < n; i++) { const p = pts[i & 63]; s += p.x * p.x + p.y * p.y; } return s; };
ops.k_field_acc = function (n) { const p = { x: 0.5, y: 0.25 }; const dx = 0.125; for (let i = 0; i < n; i++) { p.x += dx; } return p.x + p.y; };
ops.k_list_walk = function (n) { let head = null; for (let i = 63; i >= 0; i--) head = { val: i, next: head }; let s = 0, node = head; for (let i = 0; i < n; i++) { s = (s + node.val) | 0; node = node.next; if (node === null) node = head; } return s; };
ops.k_vec_local = function (n) { const a = { x: 1.5, y: 2.5 }, b = { x: 0.25, y: 0.75 }; let s = 0; for (let i = 0; i < n; i++) { const c = { x: a.x + i, y: a.y + b.y }; s += c.x * b.x + c.y; } return s; };
// Object spread of a typical state record with one field replaced (`{...state, n}`), the
// immutable-update pattern of reducers and option merging: a copy of five fields.
ops.obj_spread = function (n) { let state = { a: 1, b: 2, c: 3, d: 4, n: 0 }; for (let i = 0; i < n; i++) { state = { ...state, n: i }; } return state.n + state.a; };
// Reads of `name` and `length` from plain objects (`user.name`, `{ length }`), the names a
// function answers from its information: a plain object's are own data fields.
function nameRead(n, o) { let a = 0; for (let i = 0; i < n; i++) { a = (a + o.name) | 0; } return a; }
function lengthRead(n, o) { let a = 0; for (let i = 0; i < n; i++) { a = (a + o.length) | 0; } return a; }
ops.field_name = function (n) { return nameRead(n, { id: 3, name: 4 }); };
ops.field_length = function (n) { return lengthRead(n, { id: 3, length: 4 }); };
// Builtin method calls outside the lane's builtin rows: the native call protocol plus a small
// body (a search of a short needle, a short slice, a type test).
ops.str_includes = function (n) { let a = 0; const s = S; for (let i = 0; i < n; i++) { if (s.includes("lazy")) a++; } return a; };
ops.str_slice = function (n) { let a = 0; const s = S; for (let i = 0; i < n; i++) { a += s.slice(4, 9).length; } return a; };
ops.arr_isarray = function (n) { const arr = [1, 2, 3]; let a = 0; for (let i = 0; i < n; i++) { if (Array.isArray(arr)) a++; } return a; };
ops.arr_slice = function (n) { const arr = [1, 2, 3, 4]; let a = 0; for (let i = 0; i < n; i++) { a += arr.slice(0, 2).length; } return a; };
// Builtin bodies against their C twins (TOD-1839, rule: C-shaped code runs as C). Each budget in
// ir_budget.txt is the twin's count plus a call-dispatch allowance, not 3x C: the builtin body
// itself must cost what C costs, and the interpreter adds only the call's dispatch.
ops.bi_dv_get_f64 = function (n) { const dv = new DataView(new ArrayBuffer(64)); for (let j = 0; j < 8; j++) dv.setFloat64(j * 8, j + 0.5, true); let s = 0; for (let i = 0; i < n; i++) { s += dv.getFloat64((i & 7) * 8, true); } return s; };
ops.bi_dv_set_f64 = function (n) { const dv = new DataView(new ArrayBuffer(64)); for (let i = 0; i < n; i++) { dv.setFloat64((i & 7) * 8, i, true); } return dv.getFloat64(0, true); };
ops.bi_dv_get_u32 = function (n) { const dv = new DataView(new ArrayBuffer(64)); for (let j = 0; j < 16; j++) dv.setUint32(j * 4, j * 7); let a = 0; for (let i = 0; i < n; i++) { a = (a + dv.getUint32((i & 15) * 4)) | 0; } return a; };
ops.bi_arr_slice = function (n) { const arr = []; for (let j = 0; j < 16; j++) arr.push(j); let a = 0; for (let i = 0; i < n; i++) { a = (a + arr.slice(4, 12).length) | 0; } return a; };
ops.bi_arr_concat = function (n) { const x = [1, 2, 3, 4], y = [5, 6, 7, 8]; let a = 0; for (let i = 0; i < n; i++) { a = (a + x.concat(y).length) | 0; } return a; };
ops.bi_tmpl = function (n) { const p = "ab", q = "xyz"; let a = 0; for (let i = 0; i < n; i++) { a = (a + `${p}-${i & 1023}-${q}`.length) | 0; } return a; };
ops.bi_map_get = function (n) { const m = new Map(); for (let j = 0; j < 64; j++) m.set(j, j * 3); let a = 0; for (let i = 0; i < n; i++) { a = (a + m.get(i & 63)) | 0; } return a; };
ops.bi_set_has = function (n) { const s = new Set(); for (let j = 0; j < 64; j += 2) s.add(j); let a = 0; for (let i = 0; i < n; i++) { if (s.has(i & 63)) a++; } return a; };
ops.bi_fill = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + new Array(16).fill(0).length) | 0; } return a; };
ops.bi_join = function (n) { const parts = ["a", "bb", "ccc", "dddd"]; let a = 0; for (let i = 0; i < n; i++) { a = (a + parts.join("-").length) | 0; } return a; };
ops.bi_from = function (n) { const words = ["a", "bb", "ccc", "dddd"]; let a = 0; for (let i = 0; i < n; i++) { a = (a + Array.from(words).length) | 0; } return a; };
ops.bi_upper = function (n) { const s = "hello"; let a = 0; for (let i = 0; i < n; i++) { a = (a + s.toUpperCase().length) | 0; } return a; };
ops.bi_trim = function (n) { const s = "  abc  "; let a = 0; for (let i = 0; i < n; i++) { a = (a + s.trim().length) | 0; } return a; };
ops.bi_pad = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + String(i & 7).padStart(3, "0").length) | 0; } return a; };
ops.bi_sindex = function (n) { const s = "alpha,beta,gamma,delta"; let a = 0; for (let i = 0; i < n; i++) { a = (a + s.indexOf("gam")) | 0; } return a; };
ops.bi_sslice = function (n) { const s = "alpha,beta,gamma,delta"; let a = 0; for (let i = 0; i < n; i++) { a = (a + s.slice(6, 10).length) | 0; } return a; };
ops.bi_split = function (n) { const s = "alpha,beta,gamma,delta"; let a = 0; for (let i = 0; i < n; i++) { a = (a + s.split(",").length) | 0; } return a; };
ops.bi_replace = function (n) { const s = "alpha,beta,gamma,delta"; let a = 0; for (let i = 0; i < n; i++) { a = (a + s.replace("beta", "B").length) | 0; } return a; };
ops.bi_sort_int = function (n) { const arr16 = [11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62]; let a = 0; for (let i = 0; i < n; i++) { const keep = arr16.slice().sort(); a = (a + keep[0]) | 0; } return a; };
ops.bi_sort_cmp = function (n) { const arr16 = [11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62]; let a = 0; for (let i = 0; i < n; i++) { const keep = arr16.slice().sort((x, y) => x - y); a = (a + keep[0]) | 0; } return a; };
ops.bi_map = function (n) { const arr16 = [11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62]; let a = 0; for (let i = 0; i < n; i++) { const keep = arr16.map(x => x + 1); a = (a + keep[0]) | 0; } return a; };
ops.bi_parse = function (n) { const txt = '{"a":1,"b":[1,2,3],"c":"xyz"}'; let a = 0; for (let i = 0; i < n; i++) { a = (a + JSON.parse(txt).a) | 0; } return a; };
ops.bi_keys = function (n) { const o = { a: 1, b: 2, c: 3, d: 4 }; let a = 0; for (let i = 0; i < n; i++) { a = (a + Object.keys(o).length) | 0; } return a; };
ops.bi_assign = function (n) { const o = { a: 1, b: 2, c: 3, d: 4 }; let a = 0; for (let i = 0; i < n; i++) { a = (a + Object.assign({}, o).a) | 0; } return a; };
ops.bi_arr_at = function (n) { const arr16 = [11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62]; let a = 0; for (let i = 0; i < n; i++) { a = (a + arr16.at(i & 15)) | 0; } return a; };
ops.bi_arr_lastindexof = function (n) { const arr16 = [11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62]; let a = 0; for (let i = 0; i < n; i++) { a = (a + arr16.lastIndexOf(4)) | 0; } return a; };
ops.bi_arr_reverse = function (n) { const arr16 = [11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62]; let a = 0; for (let i = 0; i < n; i++) { arr16.reverse(); a = (a + arr16[0]) | 0; } return a; };
ops.bi_num_tofixed = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + (i * 0.5).toFixed(2).length) | 0; } return a; };
ops.bi_num_radix = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + (i & 1023).toString(16).length) | 0; } return a; };
ops.bi_getproto = function (n) { const o = { a: 1 }; let a = 0; for (let i = 0; i < n; i++) { a = (a + (Object.getPrototypeOf(o) === Object.prototype)) | 0; } return a; };
ops.bi_u8_fill = function (n) { const u8 = new Uint8Array(64); let a = 0; for (let i = 0; i < n; i++) { u8.fill(i & 255); a = (a + u8[0]) | 0; } return a; };
ops.bi_f64_indexof = function (n) { const f64 = new Float64Array(16); for (let j = 0; j < 16; j++) f64[j] = j * 0.5; let a = 0; for (let i = 0; i < n; i++) { a = (a + f64.indexOf((i & 15) * 0.5)) | 0; } return a; };
ops.bi_regexp_test = function (n) { const re = /ab+c/; let a = 0; for (let i = 0; i < n; i++) { a = (a + re.test("xxabbbc")) | 0; } return a; };
ops.bi_regexp_exec = function (n) { const re = /ab+c/; let a = 0; for (let i = 0; i < n; i++) { a = (a + re.exec("xxabbbc").index) | 0; } return a; };
ops.bi_obj_keys2 = function (n) { const o = { a: 1, b: 2 }; let a = 0; for (let i = 0; i < n; i++) { a = (a + Object.keys(o).length) | 0; } return a; };
// C-shaped constructs (inventory 2026-11-21): each has a C twin in ops.c; budget = C (ir_budget.txt).
ops.c_while = function (n) { let s = 0, i = 0; while (i < n) { s = (s + i) | 0; i++; } return s; };
ops.c_do_while = function (n) { let s = 0, i = 0; if (n > 0) { do { s = (s + i) | 0; i++; } while (i < n); } return s; };
ops.c_for_down = function (n) { let s = 0; for (let i = n - 1; i >= 0; i--) { s = (s + i) | 0; } return s; };
ops.c_loop_step2 = function (n) { let s = 0, j = 0; for (let i = 0; i < n; i++) { s = (s + j) | 0; j += 2; } return s; };
// Outer runs n >> 4 times, inner runs 16 times: total inner work is ~n.
ops.c_nested_loop = function (n) { let s = 0; const m = n >> 4; for (let i = 0; i < m; i++) { for (let j = 0; j < 16; j++) { s = (s + i * j) | 0; } } return s; };
ops.c_loop_break = function (n) { const arr = new Array(16); for (let k = 0; k < 16; k++) arr[k] = (k * 5 + 3) & 15; let s = 0, k = 0; for (let i = 0; i < n; i++) { const v = i & 15; for (k = 0; k < 16; k++) { if (arr[k] === v) break; } s = (s + k) | 0; } return s; };
ops.c_loop_continue = function (n) { let s = 0; for (let i = 0; i < n; i++) { if (i & 1) continue; s = (s + i) | 0; } return s; };
ops.c_loop_if_else = function (n) { let s = 0; for (let i = 0; i < n; i++) { if ((i & 1) === 0) s = (s + i) | 0; else s = (s - 3) | 0; } return s; };
ops.c_loop_if_chain = function (n) { let s = 0; for (let i = 0; i < n; i++) { const k = i & 3; if (k === 0) s = (s + i) | 0; else if (k === 1) s = (s ^ i) | 0; else s = (s * 3 + 1) | 0; } return s; };
ops.c_ternary = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + ((i & 1) ? 7 : i)) | 0; } return s; };
ops.c_select_minmax = function (n) { let m = 0, m2 = 1023; for (let i = 0; i < n; i++) { const v = (i * 7919) & 1023; if (v > m) m = v; if (v < m2) m2 = v; } return (m * 1024 + m2) | 0; };
ops.c_switch_dense = function (n) { let s = 0; for (let i = 0; i < n; i++) { switch (i & 7) { case 0: s = (s + 1) | 0; break; case 1: s = (s + 3) | 0; break; case 2: s = (s ^ i) | 0; break; case 3: s = (s * 3) | 0; break; case 4: s = (s - i) | 0; break; case 5: s = (s << 1) | 0; break; case 6: s = (s >> 1) ^ i; break; case 7: s = (s + 7) | 0; break; } } return s; };
ops.c_switch_sparse = function (n) { let s = 0; for (let i = 0; i < n; i++) { switch ((i & 7) * 100) { case 0: s = (s + 1) | 0; break; case 100: s = (s + 5) | 0; break; case 300: s = (s ^ i) | 0; break; case 700: s = (s * 3 + i) | 0; break; default: s = (s - 2) | 0; } } return s; };
function c_early_return_helper(arr, v) { for (let k = 0; k < 16; k++) { if (arr[k] === v) return k; } return -1; }
ops.c_early_return = function (n) { const arr = new Array(16); for (let k = 0; k < 16; k++) arr[k] = (k * 5 + 3) & 15; let s = 0; for (let i = 0; i < n; i++) { s = (s + c_early_return_helper(arr, i & 15)) | 0; } return s; };
ops.c_bool_flag = function (n) { let c = 0; for (let i = 0; i < n; i++) { const f = (i & 3) === 0; if (f && i > 10) c++; } return c; };
ops.c_logical_and_or = function (n) { let c = 0; for (let i = 0; i < n; i++) { if ((i & 1) && (i & 2) || i === 5) c++; } return c; };
ops.c_locals_rotate = function (n) { let a = 0, b = 1, c = 2; for (let i = 0; i < n; i++) { const t = a; a = b; b = c; c = (t + i) | 0; } return (a + b + c) | 0; };
ops.c_int_expr = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = ((s ^ (i * 3)) + (i >> 2) - (i & 5)) | 0; } return s; };
ops.c_arr_sum_int = function (n) { const a = new Array(256); for (let k = 0; k < 256; k++) a[k] = k * 7 - 300; let s = 0; for (let i = 0; i < n; i++) { s = (s + a[i & 255]) | 0; } return s; };
ops.c_arr_sum_dbl = function (n) { const a = new Array(256); for (let k = 0; k < 256; k++) a[k] = k + 0.5; let s = 0; for (let i = 0; i < n; i++) { s += a[i & 255]; } return s; };
ops.c_arr_store_seq = function (n) { const a = new Array(256); for (let k = 0; k < 256; k++) a[k] = 0; for (let i = 0; i < n; i++) { a[i & 255] = i; } let s = 0; for (let k = 0; k < 256; k++) s = (s + a[k]) | 0; return s; };
ops.c_arr_rmw = function (n) { const a = new Array(256); for (let k = 0; k < 256; k++) a[k] = k; for (let i = 0; i < n; i++) { a[i & 255] = (a[i & 255] + 1) | 0; } let s = 0; for (let k = 0; k < 256; k++) s = (s + a[k]) | 0; return s; };
ops.c_arr_stencil = function (n) { const a = new Array(256); for (let k = 0; k < 256; k++) a[k] = k * 3 + 1; let s = 0; for (let i = 0; i < n; i++) { const j = 1 + (i & 253); s = (s + a[j - 1] + a[j] + a[j + 1]) | 0; } return s; };
ops.c_arr_swap = function (n) { const a = new Array(256); for (let k = 0; k < 256; k++) a[k] = k; for (let i = 0; i < n; i++) { const p = i & 255, q = (i * 7) & 255; const t = a[p]; a[p] = a[q]; a[q] = t; } let s = 0; for (let k = 0; k < 256; k++) s = (s + a[k] * (k + 1)) | 0; return s; };
ops.c_arr_indirect = function (n) { const a = new Array(256), b = new Array(256); for (let k = 0; k < 256; k++) { a[k] = k * 3 + 1; b[k] = (k * 37 + 11) & 255; } let s = 0; for (let i = 0; i < n; i++) { s = (s + a[b[i & 255]]) | 0; } return s; };
ops.c_arr_histogram = function (n) { const a = new Array(256), h = new Array(16); for (let k = 0; k < 256; k++) a[k] = k * 5 + 3; for (let k = 0; k < 16; k++) h[k] = 0; for (let i = 0; i < n; i++) { h[a[i & 255] & 15]++; } let s = 0; for (let k = 0; k < 16; k++) s = (s + h[k] * (k + 1)) | 0; return s; };
ops.c_arr_2d = function (n) { const m = new Array(16); for (let r = 0; r < 16; r++) { m[r] = new Array(16); for (let c = 0; c < 16; c++) m[r][c] = r * 16 + c; } let s = 0; for (let i = 0; i < n; i++) { s = (s + m[(i >> 4) & 15][i & 15]) | 0; } return s; };
ops.c_arr_len_loop = function (n) { const a = new Array(256); for (let k = 0; k < 256; k++) a[k] = k * 9 - 50; let s = 0; /* outer n>>8, inner 256: total inner work (n>>8)*256 ~ n */ for (let o = 0; o < (n >> 8); o++) { for (let j = 0; j < a.length; j++) s = (s + a[j]) | 0; } return s; };
ops.c_i32_sum = function (n) { const t = new Int32Array(256); for (let k = 0; k < 256; k++) t[k] = k * 3 - 100; let s = 0; for (let i = 0; i < n; i++) { s = (s + t[i & 255]) | 0; } return s; };
ops.c_i32_store = function (n) { const t = new Int32Array(256); for (let i = 0; i < n; i++) { t[i & 255] = i; } let s = 0; for (let k = 0; k < 256; k++) s = (s + t[k]) | 0; return s; };
ops.c_u8_rmw = function (n) { const u = new Uint8Array(256); for (let k = 0; k < 256; k++) u[k] = k; for (let i = 0; i < n; i++) { u[i & 255] = (u[i & 255] + 3) & 255; } let s = 0; for (let k = 0; k < 256; k++) s = (s + u[k]) | 0; return s; };
ops.c_u16_load = function (n) { const u = new Uint16Array(256); for (let k = 0; k < 256; k++) u[k] = k * 257; let s = 0; for (let i = 0; i < n; i++) { s = (s + u[i & 255]) | 0; } return s; };
ops.c_f32_mad = function (n) { const f = new Float32Array(256); for (let k = 0; k < 256; k++) f[k] = k * 0.25; for (let i = 0; i < n; i++) { const k = i & 255; f[k] = f[k] * 0.5 + 1; } let s = 0; for (let k = 0; k < 256; k++) s += f[k]; return s; };
ops.c_f64_dot = function (n) { const x = new Float64Array(256), y = new Float64Array(256); for (let k = 0; k < 256; k++) { x[k] = k * 0.5; y[k] = k * 0.25 + 1; } let s = 0; for (let i = 0; i < n; i++) { const k = i & 255; s += x[k] * y[k]; } return s; };
ops.c_i32_indirect = function (n) { const idx = new Int32Array(256), d = new Int32Array(256); for (let k = 0; k < 256; k++) { idx[k] = (k * 29 + 5) & 255; d[k] = k * k - 1000; } let s = 0; for (let i = 0; i < n; i++) { s = (s + d[idx[i & 255]]) | 0; } return s; };
ops.c_struct_get2 = function (n) { const p = { x: 3, y: 4 }; let s = 0; for (let i = 0; i < n; i++) { s = (s + p.x + p.y) | 0; } return s; };
ops.c_struct_set = function (n) { const p = { x: 0, y: 0 }; for (let i = 0; i < n; i++) { p.x = i; p.y = (p.y + p.x) | 0; } return p.y; };
ops.c_struct_rmw = function (n) { const o = { count: 0, sum: 0 }; for (let i = 0; i < n; i++) { o.count = (o.count + 1) | 0; o.sum = (o.sum + i) | 0; } return (o.sum + o.count) | 0; };
ops.c_struct_array = function (n) { const pts = new Array(64); for (let j = 0; j < 64; j++) pts[j] = { x: j, y: -j }; let s = 0; for (let i = 0; i < n; i++) { s = (s + pts[i & 63].x) | 0; } return s; };
ops.c_struct_nested = function (n) { const a = { b: { c: { v: 7 } } }; let s = 0; for (let i = 0; i < n; i++) { s = (s + a.b.c.v) | 0; } return s; };
ops.c_struct_dbl = function (n) { const v = { x: 0.5, y: 1.5 }; let s = 0; for (let i = 0; i < n; i++) { s += v.x * v.y; } return s; };
ops.c_class_method = function (n) { class P { constructor(x) { this.x = x; } get() { return this.x; } } const p = new P(3); let s = 0; for (let i = 0; i < n; i++) { s = (s + p.get()) | 0; } return s; };
ops.c_class_ctor_fields = function (n) { class Q { constructor(x, y) { this.x = x; this.y = y; } add(v) { this.x = (this.x + v) | 0; } } const p = new Q(0, 1); for (let i = 0; i < n; i++) { p.add(i); } return (p.x + p.y) | 0; };
function c_call_leaf_f(a, b) { return (a + b) | 0; }
ops.c_call_leaf = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = c_call_leaf_f(s, i); } return s; };
function c_call_4args_f(a, b, c, d) { return (a + b + c + d) | 0; }
ops.c_call_4args = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = c_call_4args_f(s, i, i & 7, i >> 3); } return s; };
function c_call_2deep_g(x) { return (x + 1) | 0; }
function c_call_2deep_f(x) { return c_call_2deep_g(x); }
ops.c_call_2deep = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + c_call_2deep_f(i)) | 0; } return s; };
function c_call_void_f(o) { o.v = (o.v + 1) | 0; }
ops.c_call_void = function (n) { const o = { v: 0 }; for (let i = 0; i < n; i++) { c_call_void_f(o); } return o.v; };
function c_call_recursive_depth4_r(k, x) { if (k === 0) return (x + k) | 0; const v = c_call_recursive_depth4_r(k - 1, (x + k) | 0); return v | 0; }
ops.c_call_recursive_depth4 = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + c_call_recursive_depth4_r(4, i)) | 0; } return s; };
function c_call_dbl_f(x) { return x * x; }
ops.c_call_dbl = function (n) { let s = 0; for (let i = 0; i < n; i++) { s += c_call_dbl_f(i * 0.5); } return s; };
ops.c_closure_read = function (n) { const k = 7; const f = (x) => (x + k) | 0; let s = 0; for (let i = 0; i < n; i++) { s = (s + f(i)) | 0; } return s; };
ops.c_method_poly2 = function (n) { class A { v() { return 1; } } class B { v() { return 2; } } const objs = [new A(), new B()]; let s = 0; for (let i = 0; i < n; i++) { s = (s + objs[i & 1].v()) | 0; } return s; };
function c_return_early_f(x) { if (x < 0) return -1; if (x > 0) return 1; return 0; }
ops.c_return_early = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + c_return_early_f((i & 7) - 3)) | 0; } return s; };
ops.c_dbl_expr = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = s * 0.999 + i * 0.5 - 1.25; } return s; };
ops.c_dbl_cmp_branch = function (n) { let x = 0; for (let i = 0; i < n; i++) { x += 0.25; if (x > 100.0) x -= 100.0; } return x; };
ops.c_dbl_to_int = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + ((i * 0.37) | 0)) | 0; } return s; };
ops.c_int_to_dbl = function (n) { let d = 0; for (let i = 0; i < n; i++) { d = d + i; } return d; };
ops.c_int_div_const = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + ((i / 4) | 0)) | 0; } return s; };
ops.c_int_mod_pow2 = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + (i % 16)) | 0; } return s; };
ops.c_popcount = function (n) { let c = 0; for (let i = 0; i < n; i++) { let x = i & 1023; while (x !== 0) { x = x & (x - 1); c = (c + 1) | 0; } } return c; };
ops.c_bit_test = function (n) { let c = 0, mask = 0x55aa55aa; for (let i = 0; i < n; i++) { if (mask & (1 << (i & 31))) c++; } return c; };
ops.c_rotate = function (n) { let x = 1; for (let i = 0; i < n; i++) { x = ((x << 5) | (x >>> 27)) ^ i; } return x >>> 0; };
ops.c_hash_mix = function (n) { let h = 1; for (let i = 0; i < n; i++) { h = Math.imul(h ^ i, 0x9e3779b1) | 0; h ^= h >>> 15; } return h; };
ops.c_abs_int = function (n) { let s = 0; for (let i = 0; i < n; i++) { const x = (i & 255) - 128; s = (s + (x < 0 ? -x : x)) | 0; } return s; };
ops.c_clamp = function (n) { let s = 0; for (let i = 0; i < n; i++) { let v = (i * 7) & 1023; if (v < 100) v = 100; else if (v > 900) v = 900; s = (s + v) | 0; } return s; };
ops.c_str_char_scan = function (n) { let c = 0; for (let i = 0; i < n; i++) { c = c + (S.charCodeAt(i % 54) === 32 ? 1 : 0); } return c; };
ops.c_str_hash = function (n) { let h = 0; for (let i = 0; i < n; i++) { h = (Math.imul(h, 31) + S.charCodeAt(i % 54)) | 0; } return h; };
ops.c_str_index_eq = function (n) { let c = 0; for (let i = 0; i < n; i++) { if (S[i % 54] === "o") c++; } return c; };
ops.c_str_len_loop = function (n) { let c = 0; const outer = (n / 54) | 0; for (let i = 0; i < outer; i++) { for (let j = 0; j < S.length; j++) { if (S.charCodeAt(j) < 100) c = c + 1; } } return c; };
ops.c_counter_dec = function (n) { let s = 0, k = n; while (k > 0) { k--; s = (s + k) | 0; } return s; };
// ---- VM foundations (V8 v7 suite + JetStream 3.0 hot constructs): v_* ops, C equivalents in ops.c ----
class v_proto_method_A { constructor(k) { this.k = k; } m(x) { return (this.k + x) | 0; } }
class v_proto_method_B extends v_proto_method_A {}
ops.v_proto_method = function (n) { const b = new v_proto_method_B(3); let s = 0; for (let i = 0; i < n; i++) { s = (s + b.m(i)) | 0; } return s; };
class v_proto_chain3_field_G { constructor() { this.g = 5; } }
class v_proto_chain3_field_P extends v_proto_chain3_field_G { m(x) { return (x ^ this.g) | 0; } }
class v_proto_chain3_field_Q extends v_proto_chain3_field_P {}
class v_proto_chain3_field_R extends v_proto_chain3_field_Q { constructor() { super(); this.own = 2; } }
ops.v_proto_chain3_field = function (n) { const r = new v_proto_chain3_field_R(); let s = 0; for (let i = 0; i < n; i++) { s = (s + r.own + r.g + r.m(i)) | 0; } return s; };
class v_poly_method3_A { constructor(k) { this.k = k; } v(x) { return (x + this.k) | 0; } }
class v_poly_method3_B { constructor(k) { this.k = k; } v(x) { return (x ^ this.k) | 0; } }
class v_poly_method3_C { constructor(k) { this.k = k; } v(x) { return (x - this.k) | 0; } }
ops.v_poly_method3 = function (n) { const arr = [new v_poly_method3_A(1), new v_poly_method3_B(5), new v_poly_method3_C(2)]; let s = 0; for (let i = 0; i < n; i++) { s = (s + arr[i % 3].v(i)) | 0; } return s; };
class v_poly_method8_S0 { constructor() { this.k = 1; } v(x) { return (x + this.k) | 0; } }
class v_poly_method8_S1 { constructor() { this.a = 0; this.k = 3; } v(x) { return (x ^ this.k) | 0; } }
class v_poly_method8_S2 { constructor() { this.k = 2; this.b = 0; } v(x) { return (x - this.k) | 0; } }
class v_poly_method8_S3 { constructor() { this.a = 0; this.b = 0; this.k = 7; } v(x) { return (x + x + this.k) | 0; } }
class v_poly_method8_S4 { constructor() { this.k = 4; this.c = 0; } v(x) { return (x | this.k) | 0; } }
class v_poly_method8_S5 { constructor() { this.a = 1; this.k = 9; } v(x) { return (this.k - x) | 0; } }
class v_poly_method8_S6 { constructor() { this.k = 5; } v(x) { return (x + this.k + this.k) | 0; } }
class v_poly_method8_S7 { constructor() { this.a = 2; this.k = 2; } v(x) { return (x ^ (this.k * 3)) | 0; } }
ops.v_poly_method8 = function (n) { const objs = [new v_poly_method8_S0(), new v_poly_method8_S1(), new v_poly_method8_S2(), new v_poly_method8_S3(), new v_poly_method8_S4(), new v_poly_method8_S5(), new v_poly_method8_S6(), new v_poly_method8_S7()]; let s = 0; for (let i = 0; i < n; i++) { s = (s + objs[i & 7].v(i)) | 0; } return s; };
class v_super_call_A { constructor(k) { this.k = k; } m(x) { return (x + this.k) | 0; } }
class v_super_call_B extends v_super_call_A { m(x) { return (super.m(x) + 3) | 0; } }
ops.v_super_call = function (n) { const b = new v_super_call_B(4); let s = 0; for (let i = 0; i < n; i++) { s = (s + b.m(i)) | 0; } return s; };
class v_getter_G { constructor() { this._v = 3; } get v() { return (this._v + 1) | 0; } }
ops.v_getter = function (n) { const g = new v_getter_G(); let s = 0; for (let i = 0; i < n; i++) { s = (s + g.v) | 0; } return s; };
class v_setter_S { constructor() { this._v = 0; } set v(x) { this._v = (this._v + x) | 0; } }
ops.v_setter = function (n) { const o = new v_setter_S(); for (let i = 0; i < n; i++) { o.v = i; } return o._v; };
class v_field_poly4_P0 { constructor() { this.x = 1; this.a = 0; } }
class v_field_poly4_P1 { constructor() { this.b = 0; this.x = 2; } }
class v_field_poly4_P2 { constructor() { this.x = 3; this.c = 0; this.d = 0; } }
class v_field_poly4_P3 { constructor() { this.e = 0; this.f = 0; this.x = 4; } }
ops.v_field_poly4 = function (n) { const objs = [new v_field_poly4_P0(), new v_field_poly4_P1(), new v_field_poly4_P2(), new v_field_poly4_P3()]; let s = 0; for (let i = 0; i < n; i++) { s = (s + objs[i & 3].x) | 0; } return s; };
class v_instanceof_A {}
class v_instanceof_B extends v_instanceof_A {}
class v_instanceof_C {}
ops.v_instanceof = function (n) { const objs = [new v_instanceof_A(), new v_instanceof_B(), new v_instanceof_C(), {}]; let c = 0; for (let i = 0; i < n; i++) { if (objs[i & 3] instanceof v_instanceof_A) c = (c + 1) | 0; } return c; };
class v_new_point_Point { constructor(x, y) { this.x = x; this.y = y; } }
ops.v_new_point = function (n) { let s = 0; for (let i = 0; i < n; i++) { const p = new v_new_point_Point(i, i + 1); s = (s + p.x + p.y) | 0; } return s; };
ops.v_obj_literal = function (n) { let s = 0; for (let i = 0; i < n; i++) { const o = { a: i, b: i + 1, c: i & 7 }; s = (s + o.a + o.b - o.c) | 0; } return s; };
class v_linked_list_walk_Node { constructor(v) { this.v = v; this.next = null; } }
ops.v_linked_list_walk = function (n) { const nodes = []; for (let k = 0; k < 256; k++) nodes.push(new v_linked_list_walk_Node((k * 7 + 3) & 255)); for (let k = 0; k < 256; k++) nodes[k].next = nodes[(k + 1) & 255]; let node = nodes[0], s = 0; for (let i = 0; i < n; i++) { s = (s + node.v) | 0; node = node.next; } return s; };
ops.v_closure_call = function (n) { let a = 3, b = 5; const f = (x) => (x + a + b) | 0; let s = 0; for (let i = 0; i < n; i++) { s = (s + f(i)) | 0; } return s; };
ops.v_closure_counter = function (n) { let cnt = 0; const bump = (k) => { cnt = (cnt + k) | 0; }; for (let i = 0; i < n; i++) { bump(i & 3); } return cnt; };
function v_bind_call_getk(x) { return (this.k + x) | 0; }
ops.v_bind_call = function (n) { const f = v_bind_call_getk.bind({ k: 3 }); let s = 0; for (let i = 0; i < n; i++) { s = (s + f(i)) | 0; } return s; };
function v_fn_call_f(x) { return (this.k * 2 + x) | 0; }
ops.v_fn_call = function (n) { const o = { k: 3 }; let s = 0; for (let i = 0; i < n; i++) { s = (s + v_fn_call_f.call(o, i)) | 0; } return s; };
function v_fn_apply_f(a, b) { return (this.k + a + b) | 0; }
ops.v_fn_apply = function (n) { const o = { k: 3 }; const arr2 = [0, 4]; let s = 0; for (let i = 0; i < n; i++) { arr2[0] = i & 255; s = (s + v_fn_apply_f.apply(o, arr2)) | 0; } return s; };
function v_arguments_length_f(a) { return (arguments.length + arguments[0]) | 0; }
ops.v_arguments_length = function (n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + v_arguments_length_f(i, 7)) | 0; } return s; };
class v_richards_task_T { constructor() { this.state = 0; this.acc = 0; this.count = 0; } step(x) { switch (this.state) { case 0: this.acc = (this.acc + x) | 0; this.state = 1; break; case 1: this.acc = (this.acc ^ x) | 0; this.state = 2; break; case 2: this.acc = (this.acc - x) | 0; this.state = 3; break; default: this.acc = (this.acc + x + x) | 0; this.state = 0; } this.count = (this.count + 1) | 0; return this.acc; } }
ops.v_richards_task = function (n) { const t = new v_richards_task_T(); for (let i = 0; i < n; i++) { t.step(i); } return (t.acc + t.count) | 0; };
class v_deltablue_constraint_Var { constructor(v) { this.value = v; } }
class v_deltablue_constraint_Cons { constructor(a, b) { this.v1 = a; this.v2 = b; } }
class v_deltablue_constraint_Eq extends v_deltablue_constraint_Cons { execute() { this.v2.value = this.v1.value; } }
class v_deltablue_constraint_Scale extends v_deltablue_constraint_Cons { execute() { this.v2.value = (this.v1.value + this.v1.value + 1) | 0; } }
class v_deltablue_constraint_Xor extends v_deltablue_constraint_Cons { execute() { this.v2.value = (this.v1.value ^ 5) | 0; } }
ops.v_deltablue_constraint = function (n) { const a = new v_deltablue_constraint_Var(0), b = new v_deltablue_constraint_Var(0); const cs = [new v_deltablue_constraint_Eq(a, b), new v_deltablue_constraint_Scale(a, b), new v_deltablue_constraint_Xor(a, b)]; let s = 0; for (let i = 0; i < n; i++) { a.value = i; cs[i % 3].execute(); s = (s + b.value) | 0; } return s; };
// --- ob group: arrays and numeric kernels (NavierStokes, Crypto, Box2D, gaussian-blur, hash-map, sha256, float-mm, n-body, stanford-crypto) ---
ops.v_array_push_pop = function (n) { const arr = [1, 2, 3, 4]; let a = 0; for (let i = 0; i < n; i++) { arr.push(i ^ a); a = (a + arr.pop() + arr.length) | 0; } return a; };
// Outer n/16 new arrays, 16 pushes each (total ~n pushes).
ops.v_array_push_grow = function (n) { let a = 0; const outer = n >> 4; for (let j = 0; j < outer; j++) { const arr = []; for (let k = 0; k < 16; k++) arr.push(k + j); a = (a + arr[15] + arr.length) | 0; } return a; };
// Array of arrays: 16 rows of 16; read grid[i>>4][i&15] with the row index masked to 0..15.
ops.v_array_2d = function (n) { const grid = []; for (let r = 0; r < 16; r++) { const row = []; for (let c = 0; c < 16; c++) row.push(r * 16 + c); grid.push(row); } let a = 0; for (let i = 0; i < n; i++) { a = (a + grid[(i >> 4) & 15][i & 15]) | 0; } return a; };
// One lin_solve-style Gauss-Seidel stencil update per iteration on an 18x18 Float64Array grid (16x16 interior); exactly n updates, cell = i & 255.
ops.v_navier_lin_solve = function (n) { const x = new Float64Array(324), x0 = new Float64Array(324); for (let k = 0; k < 324; k++) { x0[k] = ((k * 37) % 101) * 0.25; } const a = 0.1, c = 1 + 4 * a; for (let i = 0; i < n; i++) { const cell = i & 255; const id = ((cell >> 4) + 1) * 18 + (cell & 15) + 1; const s = x[id - 18] + x[id + 18] + x[id - 1] + x[id + 1]; x[id] = (x0[id] + a * s) / c; } let r = 0; for (let k = 0; k < 324; k++) { r += Math.floor(x[k] * 4096); } return r; };
ops.v_f64_axpy = function (n) { const x = new Float64Array(256), y = new Float64Array(256); for (let k = 0; k < 256; k++) { x[k] = k * 0.25; y[k] = 1; } const a = 0.5; for (let i = 0; i < n; i++) { const j = i & 255; y[j] = a * x[j] + y[j]; } let s = 0; for (let k = 0; k < 256; k++) { s += y[k]; } return s; };
ops.v_u8_bytes = function (n) { const b = new Uint8Array(256); for (let k = 0; k < 256; k++) { b[k] = k; } for (let i = 0; i < n; i++) { const j = (i * 7) & 255; b[j] = b[j] ^ 0x5a; } let s = 0; for (let k = 0; k < 256; k++) { s = (s + b[k]) | 0; } return s; };
// Tom Wu BigInteger am3 inner loop (28-bit digits split at 14 bits); 16 digits per call, n/16 calls.
function v_crypto_am3_step(a, w, x, c, n) { const xl = x & 0x3fff, xh = x >>> 14; let i = 0, j = 0; while (--n >= 0) { let l = a[i] & 0x3fff; const h = a[i++] >>> 14; const m = xh * l + h * xl; l = xl * l + ((m & 0x3fff) << 14) + w[j] + c; c = (l >>> 28) + (m >>> 14) + xh * h; w[j++] = l & 0xfffffff; } return c; }
ops.v_crypto_am3 = function (n) { const a = new Int32Array(16), w = new Int32Array(16); for (let k = 0; k < 16; k++) { a[k] = (k * 2654435 + 12345) & 0xfffffff; w[k] = (k * 40503 + 7) & 0xfffffff; } let s = 0; const outer = n >> 4; for (let r = 0; r < outer; r++) { s = (s + v_crypto_am3_step(a, w, (r * 7919 + 3) & 0xfffffff, 0, 16)) | 0; } for (let k = 0; k < 16; k++) { s = (s + w[k]) | 0; } return s; };
// One SHA-256 style round per iteration on int32 locals: Sigma1, Ch, Sigma0, Maj, rotr via >>> and <<, message word from i.
ops.v_sha_round = function (n) { let a = 0x6a09e667 | 0, b = 0xbb67ae85 | 0, c = 0x3c6ef372 | 0, d = 0xa54ff53a | 0, e = 0x510e527f | 0, f = 0x9b05688c | 0, g = 0x1f83d9ab | 0, h = 0x5be0cd19 | 0; for (let i = 0; i < n; i++) { const w = (Math.imul(i, 0x85ebca6b) + 0x27d4eb2f) | 0; const S1 = ((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7)); const ch = (e & f) ^ (~e & g); const t1 = (h + S1 + ch + 0x428a2f98 + w) | 0; const S0 = ((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10)); const maj = (a & b) ^ (a & c) ^ (b & c); const t2 = (S0 + maj) | 0; h = g; g = f; f = e; e = (d + t1) | 0; d = c; c = b; b = a; a = (t1 + t2) | 0; } return (a ^ b ^ c ^ d ^ e ^ f ^ g ^ h) | 0; };
// One pair interaction of two bodies per iteration: doubles, Math.sqrt, symplectic-Euler step; softened by eps.
ops.v_nbody_step = function (n) { let x1 = 1.0, y1 = 0.0, z1 = 0.0, vx1 = 0.0, vy1 = 0.5, vz1 = 0.0; let x2 = -1.0, y2 = 0.1, z2 = 0.0, vx2 = 0.0, vy2 = -0.5, vz2 = 0.0; const m1 = 1.0, m2 = 0.5, dt = 0.001, eps = 0.1; for (let i = 0; i < n; i++) { const dx = x1 - x2, dy = y1 - y2, dz = z1 - z2; const d2 = dx * dx + dy * dy + dz * dz + eps; const mag = dt / (d2 * Math.sqrt(d2)); vx1 = vx1 - dx * m2 * mag; vy1 = vy1 - dy * m2 * mag; vz1 = vz1 - dz * m2 * mag; vx2 = vx2 + dx * m1 * mag; vy2 = vy2 + dy * m1 * mag; vz2 = vz2 + dz * m1 * mag; x1 = x1 + dt * vx1; y1 = y1 + dt * vy1; z1 = z1 + dt * vz1; x2 = x2 + dt * vx2; y2 = y2 + dt * vy2; z2 = z2 + dt * vz2; } return Math.floor((x1 + y1 + z1 + x2 + y2 + z2) * 1000); };
// 3x3 double matrix multiply-accumulate C += A*B; n/9 iterations (each 9 output entries, 27 mul-adds).
ops.v_float_mm = function (n) { const A = new Float64Array(9), B = new Float64Array(9), C = new Float64Array(9); for (let k = 0; k < 9; k++) { A[k] = (k + 1) * 0.5; B[k] = (9 - k) * 0.25; C[k] = 0; } const iters = (n / 9) | 0; for (let i = 0; i < iters; i++) { for (let r = 0; r < 3; r++) for (let c = 0; c < 3; c++) { C[r * 3 + c] += A[r * 3] * B[c] + A[r * 3 + 1] * B[3 + c] + A[r * 3 + 2] * B[6 + c]; } } let s = 0; for (let k = 0; k < 9; k++) { s = s + C[k]; } return s; };
ops.v_int_to_double = function (n) { let a = 0.25; for (let i = 0; i < n; i++) { const k = i & 1023; a = a + (k * 0.5 - i * 0.25); } return a; };
// Math.sin + Math.cos per iteration, quantized to an integer per term so the checksum is exact.
ops.v_math_sin_cos = function (n) { let a = 0; for (let i = 0; i < n; i++) { const x = i * 0.001; a = (a + Math.floor((Math.sin(x) + Math.cos(x)) * 1024)) | 0; } return a; };
// forEach over a 16-array with a closure callback; outer n/16 calls, so n/16*16 ~ n element visits.
ops.v_array_foreach = function (n) { const arr = []; for (let k = 0; k < 16; k++) { arr.push(k); } let s = 0; const outer = n >> 4; for (let r = 0; r < outer; r++) { arr.forEach((x) => { s = (s + x) | 0; }); } return s; };
// map over a 16-array, result read back; outer n/16 calls (~n element visits).
ops.v_array_map = function (n) { const arr = []; for (let k = 0; k < 16; k++) { arr.push(k); } let s = 0; const outer = n >> 4; for (let r = 0; r < outer; r++) { const m = arr.map((x) => x * 2); s = (s + m[r & 15]) | 0; } return s; };
// reduce sum over a 16-array; outer n/16 calls (~n element visits).
ops.v_array_reduce = function (n) { const arr = []; for (let k = 0; k < 16; k++) { arr.push(k * 3 + 1); } let s = 0; const outer = n >> 4; for (let r = 0; r < outer; r++) { s = (s + arr.reduce((p, x) => (p + x) | 0, 0)) | 0; } return s; };
// indexOf on a 16-int array; keys cycle 0..63, so about a quarter hit and the rest return -1.
ops.v_array_indexof = function (n) { const arr = []; for (let k = 0; k < 16; k++) { arr.push(k * 3); } let s = 0; for (let i = 0; i < n; i++) { s = (s + arr.indexOf(i & 63)) | 0; } return s; };
// Holey array: slots 1,3,5,... never set; reading a hole gives undefined, which |0 turns into 0.
ops.v_array_holey = function (n) { const arr = new Array(32); for (let k = 0; k < 32; k += 2) { arr[k] = k; } let s = 0; for (let i = 0; i < n; i++) { s = (s + (arr[i & 31] | 0)) | 0; } return s; };
ops.v_array_of_objects_field = function (n) { const arr = []; for (let k = 0; k < 256; k++) { arr.push({ v: k }); } let s = 0; for (let i = 0; i < n; i++) { s = (s + arr[i & 255].v) | 0; } return s; };
// Int32Array built from a 16-element plain array, then .sort(); outer n/16 iterations with changing input.
ops.v_typed_from_array_sort = function (n) { const base = new Array(16); let s = 0; const outer = n >> 4; for (let r = 0; r < outer; r++) { for (let k = 0; k < 16; k++) { base[k] = (k * 73 + r * 29) & 255; } const t = new Int32Array(base); t.sort(); s = (s + t[r & 15]) | 0; } return s; };
// 1D 5-tap blur on Float32Array (wrap-around indices), one output sample per iteration; weights 1/16 4/16 6/16 4/16 1/16.
ops.v_gaussian_blur = function (n) { const src = new Float32Array(256), dst = new Float32Array(256); for (let k = 0; k < 256; k++) { src[k] = (k * 37) & 255; } for (let i = 0; i < n; i++) { const j = i & 255; dst[j] = 0.0625 * src[(j + 254) & 255] + 0.25 * src[(j + 255) & 255] + 0.375 * src[j] + 0.25 * src[(j + 1) & 255] + 0.0625 * src[(j + 2) & 255]; } let s = 0; for (let k = 0; k < 256; k++) { s = s + dst[k]; } return s; };
const v_str_charcode_scan_SRC = "let x1 = foo(bar,42) + baz[7] * 3.5; return x1 >= 10 && yy || z;";
const v_str_split_join_SRC = "alpha beta gamma delta";
const v_str_indexof_SRC = "the quick brown fox jumps over the lazy dog 0123456789";
const v_str_slice_SRC = "the quick brown fox jumps over the lazy dog 0123456789";
const v_str_replace_regexp_SRC = "the quick brown fox jumps over the lazy dog";
const v_parse_int_TAB = ["12", "7", "301", "45", "9", "1024", "88", "3", "560", "21", "6", "777", "42", "100", "5", "2048"];
const v_parse_float_TAB = ["3.25", "0.5", "12.75", "100.125", "7.0", "0.25", "2.5", "64.5", "1024.0625", "9.75", "0.125", "5", "33.5", "18.25", "2.0", "0.75"];
const v_regexp_test_TAB = ["abc123", "xyz", "a1", "hello world", "foo42", "BAR7", "q", "z99x", "pq0", "12ab", "mno789", "k", "ab-12", "go0go", "x_1", "tuv2024"];
const v_regexp_exec_groups_TAB = ["key=12", "alpha=7", "x=100", "name=3", "val=4096", "k=0", "abc=99", "n=5"];
const v_json_parse_SRC = '{"a":12,"b":[1,2,3]}';
const v_str_key_dict_KEYS = ["red", "green", "blue", "cyan", "gold", "pink", "navy", "teal", "lime", "gray", "rose", "sand", "plum", "jade", "ruby", "mint"];
const v_str_concat3_PARTS = ["ab", "cde", "f", "ghij"];
// Each op below: n iterations of its construct (no nested loops).
ops.v_str_build = function (n) { let a = 0, s = ""; for (let i = 0; i < n; i++) { s += "ab"; if (s.length >= 64) { a += s.length; s = ""; } } return a + s.length; };
ops.v_str_charcode_scan = function (n) { const src = v_str_charcode_scan_SRC; let id = 0, dg = 0, sp = 0, ot = 0; for (let i = 0; i < n; i++) { const c = src.charCodeAt(i & 63); if (c >= 97 && c <= 122) id++; else if (c >= 48 && c <= 57) dg++; else if (c === 32) sp++; else ot++; } return id + dg * 3 + sp * 5 + ot * 7; };
ops.v_str_fromcharcode = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + String.fromCharCode(65 + (i & 15)).length) | 0; } return a; };
ops.v_str_indexof = function (n) { const s = v_str_indexof_SRC, nd = "dog"; let a = 0; for (let i = 0; i < n; i++) { a = (a + s.indexOf(nd)) | 0; } return a; };
ops.v_str_slice = function (n) { const s = v_str_slice_SRC; let a = 0; for (let i = 0; i < n; i++) { const k = i & 15; a = (a + s.slice(k, k + 8).length + s.charCodeAt(0)) | 0; } return a; };
// Per iteration: one split into 4 parts and one join of them (n splits, n joins).
ops.v_str_split_join = function (n) { const s = v_str_split_join_SRC; let a = 0; for (let i = 0; i < n; i++) { const parts = s.split(" "); const j = parts.join("-"); a = (a + j.length + parts.length) | 0; } return a; };
ops.v_str_template = function (n) { let a = 0; for (let i = 0; i < n; i++) { const t = `${i & 255}-${i & 15}`; a = (a + t.length) | 0; } return a; };
ops.v_str_compare_lt = function (n) { let c = 0; const a = "abcdefgh", b = "abcdefgi"; for (let i = 0; i < n; i++) { if (a < b) c++; } return c; };
ops.v_str_tolower = function (n) { const s = "ABCDEFGH"; let a = 0; for (let i = 0; i < n; i++) { const t = s.toLowerCase(); a = (a + t.charCodeAt(i & 7)) | 0; } return a; };
ops.v_str_eq_dynamic = function (n) { const x = ["ab", "cd", "ef", "gh"].join(""), y = ["ab", "cd", "ef", "gh"].join(""); let c = 0; for (let i = 0; i < n; i++) { if (x === y) c++; } return c; };
ops.v_num_tostring = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + String(i & 1023).length) | 0; } return a; };
ops.v_parse_int = function (n) { const t = v_parse_int_TAB; let a = 0; for (let i = 0; i < n; i++) { a = (a + parseInt(t[i & 15], 10)) | 0; } return a; };
ops.v_parse_float = function (n) { const t = v_parse_float_TAB; let a = 0; for (let i = 0; i < n; i++) { a += parseFloat(t[i & 15]); } return a; };
ops.v_regexp_test = function (n) { const t = v_regexp_test_TAB, re = /^[a-z]+\d+$/; let c = 0; for (let i = 0; i < n; i++) { if (re.test(t[i & 15])) c++; } return c; };
ops.v_regexp_exec_groups = function (n) { const t = v_regexp_exec_groups_TAB, re = /(\w+)=(\d+)/; let a = 0; for (let i = 0; i < n; i++) { const m = re.exec(t[i & 7]); a = (a + m[1].length + (m[2] | 0)) | 0; } return a; };
ops.v_str_replace_regexp = function (n) { const s = v_str_replace_regexp_SRC, re = /o/g; let a = 0; for (let i = 0; i < n; i++) { a = (a + s.replace(re, "0").length) | 0; } return a; };
ops.v_json_stringify = function (n) { const o = { a: 0, b: "str", c: [1, 2] }; let a = 0; for (let i = 0; i < n; i++) { o.a = i & 1023; a = (a + JSON.stringify(o).length) | 0; } return a; };
ops.v_json_parse = function (n) { const src = v_json_parse_SRC; let a = 0; for (let i = 0; i < n; i++) { const o = JSON.parse(src); a = (a + o.a + o.b[0] + o.b[1] + o.b[2]) | 0; } return a; };
ops.v_str_key_dict = function (n) { const keys = v_str_key_dict_KEYS, d = {}; for (let j = 0; j < 16; j++) d[keys[j]] = j * 3 + 1; let a = 0; for (let i = 0; i < n; i++) { a = (a + d[keys[i & 15]]) | 0; } return a; };
ops.v_str_concat3 = function (n) { const P = v_str_concat3_PARTS; let s = 0; for (let i = 0; i < n; i++) { const t = P[i & 3] + "-" + P[(i >> 2) & 3]; s += t.length; } return s; };
const v_map_get_str_keys = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi"];
const v_switch_string_names = ["add", "sub", "mul", "div", "mod", "and", "or", "xor"];
function* v_generator_next_gen() { let x = 0; while (true) { yield x; x = (x + 3) | 0; } }
function v_throw_helper(i) { if ((i & 15) === 0) throw new Error("v_throw"); return i; }
function v_spread_f(x, y, z) { return (x + y + z) | 0; }
function v_rest_g(...r) { return (r.length + r[0]) | 0; }
function v_destructure_obj_run(n, o) { let a = 0; for (let i = 0; i < n; i++) { const { x, y } = o; a = (a + x + y + (i & 1)) | 0; } return a; }
function v_optional_chain_run(n, objs) { let a = 0; for (let i = 0; i < n; i++) { const o = objs[i & 3]; const v = o?.p?.q; a = (a + (v === undefined ? 1 : v)) | 0; } return a; }
function v_splay_build() {
  const root = { key: (0 * 97) & 255, left: null, right: null };
  for (let j = 1; j < 256; j++) {
    const key = (j * 97) & 255; let cur = root;
    while (true) {
      if (key < cur.key) { if (cur.left === null) { cur.left = { key, left: null, right: null }; break; } cur = cur.left; }
      else { if (cur.right === null) { cur.right = { key, left: null, right: null }; break; } cur = cur.right; }
    }
  }
  return root;
}
function v_heap_push(h, size, v) { let k = size; h[k] = v; while (k > 0) { const p = (k - 1) >> 1; if (h[p] <= h[k]) break; const t = h[p]; h[p] = h[k]; h[k] = t; k = p; } }
function v_heap_pop(h, size) { const top = h[0]; const m = size - 1; h[0] = h[m]; let k = 0; while (true) { const l = 2 * k + 1; if (l >= m) break; let c = l; if (l + 1 < m && h[l + 1] < h[l]) c = l + 1; if (h[k] <= h[c]) break; const t = h[k]; h[k] = h[c]; h[c] = t; k = c; } return top; }
ops.v_map_get_int = function (n) { const m = new Map(); for (let j = 0; j < 256; j++) m.set(j, j * 3); let a = 0; for (let i = 0; i < n; i++) { a = (a + m.get(i & 255)) | 0; } return a; };
ops.v_map_set_delete = function (n) { const m = new Map(); let a = 0; for (let i = 0; i < n; i++) { const k = i & 255; m.set(k, i); if (m.delete(k)) a = (a + 1) | 0; } return a; };
ops.v_map_get_str = function (n) { const m = new Map(); for (let j = 0; j < 16; j++) m.set(v_map_get_str_keys[j], j * 5 + 1); let a = 0; for (let i = 0; i < n; i++) { a = (a + m.get(v_map_get_str_keys[i & 15])) | 0; } return a; };
ops.v_set_has = function (n) { const s = new Set(); for (let j = 0; j < 256; j++) s.add(j * 3); let a = 0; for (let i = 0; i < n; i++) { if (s.has(i & 1023)) a = (a + 1) | 0; } return a; };
ops.v_map_iterate = function (n) { const m = new Map(); for (let j = 0; j < 16; j++) m.set(j, j + 1); let a = 0; for (let i = 0; i < (n >> 4); i++) { for (const [k, v] of m) { a = (a + k + v) | 0; } } return a; };
ops.v_obj_keys_loop = function (n) { const o = { alpha: 1, beta: 2, gamma: 3, delta: 4 }; let a = 0; for (let i = 0; i < n; i++) { const ks = Object.keys(o); for (let j = 0; j < ks.length; j++) { a = (a + ks[j].length) | 0; } } return a; };
ops.v_for_in = function (n) { const o = { alpha: 1, beta: 2, gamma: 3, delta: 4 }; let a = 0; for (let i = 0; i < (n >> 2); i++) { for (const k in o) { a = (a + k.length + o[k]) | 0; } } return a; };
ops.v_for_of_array = function (n) { const arr = []; for (let j = 0; j < 16; j++) arr.push(j * 7 + 1); let a = 0; for (let i = 0; i < (n >> 4); i++) { for (const x of arr) { a = (a + x) | 0; } } return a; };
ops.v_generator_next = function (n) { const g = v_generator_next_gen(); let a = 0; for (let i = 0; i < n; i++) { a = (a + g.next().value) | 0; } return a; };
ops.v_try_nothrow = function (n) { let a = 0; for (let i = 0; i < n; i++) { try { a = (a + (i ^ 5)) | 0; } catch (e) { } } return a; };
ops.v_throw_catch = function (n) { let a = 0; for (let i = 0; i < n; i++) { try { a = (a + v_throw_helper(i)) | 0; } catch (e) { a = (a + 1) | 0; } } return a; };
ops.v_spread_call = function (n) { const arr3 = [0, 2, 3]; let a = 0; for (let i = 0; i < n; i++) { arr3[0] = i; a = (a + v_spread_f(...arr3)) | 0; } return a; };
ops.v_rest_args = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + v_rest_g(i, 2, 3)) | 0; } return a; };
ops.v_destructure_array = function (n) { let a = 0, b = 1; for (let i = 0; i < n; i++) { [a, b] = [b, (a + i) | 0]; } return (a + b) | 0; };
ops.v_destructure_obj = function (n) { return v_destructure_obj_run(n, { x: 3, y: 4 }); };
ops.v_optional_chain = function (n) { return v_optional_chain_run(n, [{ p: { q: 5 } }, null, { p: null }, { p: { q: 9 } }]); };
ops.v_nullish = function (n) { let a = 0; for (let i = 0; i < n; i++) { const x = (i & 1) ? null : i; a = (a + (x ?? 3)) | 0; } return a; };
ops.v_splay_insert_find = function (n) { const root = v_splay_build(); let cur = root, want = 0, a = 0; for (let i = 0; i < n; i++) { if (cur.key === want) { a = (a + cur.key) | 0; want = (want + 37) & 255; cur = root; } else { cur = want < cur.key ? cur.left : cur.right; } } return a; };
ops.v_astar_heap = function (n) { const h = new Array(64).fill(0); let size = 0, a = 0; for (let i = 0; i < n; i++) { if (size === 0 || (size < 60 && (i % 3) !== 2)) { v_heap_push(h, size, (i * 73 + 17) & 4095); size++; } else { a = (a + v_heap_pop(h, size)) | 0; size--; } } return a; };
ops.v_switch_string = function (n) { let acc = 1; for (let i = 0; i < n; i++) { switch (v_switch_string_names[i & 7]) { case "add": acc = (acc + 1) | 0; break; case "sub": acc = (acc - 3) | 0; break; case "mul": acc = (acc * 3) | 0; break; case "div": acc = (acc / 2) | 0; break; case "mod": acc = (acc % 1009) | 0; break; case "and": acc = acc & 0xffff; break; case "or": acc = acc | 0x55; break; case "xor": acc = acc ^ 0x5a5a; break; } } return acc; };
// ---- oe group: allocation, GC churn and remaining foundations (V8 v7 suite, JetStream 3.0) ----
// Nested loops: the outer loop runs n/16 or n/4 times so the inner work totals about n.
class v_vec3_ops_Vec { constructor(x, y, z) { this.x = x; this.y = y; this.z = z; } add(o) { return new v_vec3_ops_Vec(this.x + o.x, this.y + o.y, this.z + o.z); } scale(k) { return new v_vec3_ops_Vec(this.x * k, this.y * k, this.z * k); } dot(o) { return this.x * o.x + this.y * o.y + this.z * o.z; } }
class v_box2d_vec2_Vec2 { constructor(x, y) { this.x = x; this.y = y; } addScaled(o, k) { this.x += o.x * k; this.y += o.y * k; return this; } set(x, y) { this.x = x; this.y = y; return this; } }
class v_class_hierarchy_ctor_A { constructor(i) { this.a = i; } }
class v_class_hierarchy_ctor_B extends v_class_hierarchy_ctor_A { constructor(i) { super(i); this.b = i + 1; } }
class v_class_hierarchy_ctor_C extends v_class_hierarchy_ctor_B { constructor(i) { super(i); this.c = i + 2; } }
ops.v_alloc_small = function (n) { let s = 0; for (let i = 0; i < n; i++) { const o = { v: i }; s = (s + o.v) | 0; } return s; };
ops.v_alloc_array4 = function (n) { let s = 0; for (let i = 0; i < n; i++) { const arr = [i, i + 1, i + 2, i + 3]; s = (s + arr[0] + arr[1] + arr[2] + arr[3]) | 0; } return s; };
ops.v_alloc_closure = function (n) { let s = 0; for (let i = 0; i < n; i++) { const k = i & 7; const f = (x) => (x + i + k) | 0; s = (s + f(k)) | 0; } return s; };
ops.v_alloc_retained = function (n) { const ring = new Array(1024); for (let j = 0; j < 1024; j++) ring[j] = { a: 0, b: 0 }; let s = 0; for (let i = 0; i < n; i++) { ring[i & 1023] = { a: i, b: i + 1 }; s = (s + ring[(i + 512) & 1023].b) | 0; } return s; };
ops.v_cons_list = function (n) { const outer = n >> 4; let s = 0; for (let i = 0; i < outer; i++) { let l = null; for (let j = 0; j < 16; j++) l = { car: i + j, cdr: l }; for (let p = l; p !== null; p = p.cdr) s = (s + p.car) | 0; } return s; };
ops.v_vec3_ops = function (n) { let s = 0; for (let i = 0; i < n; i++) { const a = new v_vec3_ops_Vec(i, 1, 2); const b = a.add(new v_vec3_ops_Vec(1, 2, 3)).scale(0.5); s += a.dot(b); } return s; };
ops.v_box2d_vec2 = function (n) { const p = new v_box2d_vec2_Vec2(0, 0), vel = new v_box2d_vec2_Vec2(1.5, -0.25); let s = 0; for (let i = 0; i < n; i++) { p.addScaled(vel, 0.5); if (p.x > 4096) p.set(0, p.y); s += p.x + p.y; } return s; };
ops.v_class_hierarchy_ctor = function (n) { let s = 0; for (let i = 0; i < n; i++) { const o = new v_class_hierarchy_ctor_C(i); s = (s + o.a + o.b + o.c) | 0; } return s; };
ops.v_array_literal_objects = function (n) { const outer = n >> 2; let s = 0; for (let i = 0; i < outer; i++) { const arr = [{ x: i }, { x: i + 1 }, { x: i + 2 }, { x: i + 3 }]; for (let j = 0; j < 4; j++) s = (s + arr[j].x) | 0; } return s; };
ops.v_object_assign = function (n) { const src = { a: 1, b: 2, c: 3 }; let s = 0; for (let i = 0; i < n; i++) { src.b = i; const o = Object.assign({}, src); s = (s + o.a + o.b + o.c) | 0; } return s; };
ops.v_object_spread = function (n) { const src = { a: 1, b: 2, c: 3 }; let s = 0; for (let i = 0; i < n; i++) { src.b = i; const r = { ...src, d: i }; s = (s + r.a + r.b + r.c + r.d) | 0; } return s; };
ops.v_string_keys_obj_build = function (n) { const KEYS = ["alpha", "beta", "gamma", "delta"]; let s = 0; for (let i = 0; i < n; i++) { const o = {}; for (let j = 0; j < 4; j++) o[KEYS[j]] = i + j; s = (s + o[KEYS[i & 3]]) | 0; } return s; };
ops.v_bigint_add = function (n) { let a = 1n; const k = 0x9e3779b97f4a7c15n; for (let i = 0; i < n; i++) { a = BigInt.asIntN(64, a + k + BigInt(i)); } return Number(BigInt.asUintN(32, a)); };
ops.v_int53_arith = function (n) { let a = 1; for (let i = 0; i < n; i++) { a = (a * 31 + i) % 2147483647; } return a; };
ops.v_dataview_get = function (n) { const buf = new ArrayBuffer(64); const dv = new DataView(buf); for (let j = 0; j < 16; j++) dv.setUint32(j * 4, (j * 2654435761) >>> 0, true); let s = 0; for (let i = 0; i < n; i++) { s = (s + dv.getUint32((i & 15) * 4, true)) | 0; } return s; };
ops.v_unipoker_rank = function (n) { const POP = new Array(256); for (let k = 0; k < 256; k++) { let c = 0, x = k; while (x !== 0) { x &= x - 1; c++; } POP[k] = c; } let s = 0; for (let i = 0; i < n; i++) { let ranks = 0, suits = 0; for (let k = 0; k < 5; k++) { const card = (i * 7 + k * k * 11 + (i >> 3) * k) % 52; ranks |= 1 << (card >> 2); suits |= 1 << (card & 3); } const distinct = POP[ranks & 255] + POP[(ranks >> 8) & 255]; const straight = (ranks & (ranks >> 1) & (ranks >> 2) & (ranks >> 3) & (ranks >> 4)) !== 0 ? 1 : 0; const flush = (suits & (suits - 1)) === 0 ? 1 : 0; s = (s + (flush * 2 + straight) * 8 + distinct) | 0; } return s; };
ops.v_basic_interp = function (n) { const code = [0, 2, 1, 3, 3, 0, 1, 2, 2, 1, 0, 3, 1, 1, 2, 0]; let acc = 1, pc = 0; for (let i = 0; i < n; i++) { switch (code[pc]) { case 0: acc = (acc + 1) | 0; break; case 1: acc = acc ^ 0x5a; break; case 2: acc = (acc * 3) | 0; break; case 3: acc = acc >> 1; break; } pc = (pc + 1) & 15; } return acc; };
ops.v_air_regalloc_like = function (n) { const objs = []; for (let j = 0; j < 64; j++) objs.push({ a: j, b: j * 2, c: 0 }); let s = 0; for (let i = 0; i < n; i++) { const o = objs[i & 63]; o.c = (o.a + o.b) | 0; s = (s + o.c) | 0; } return s; };
ops.v_ml_dot_sigmoid = function (n) { const w = [0.5, -0.25, 0.125, 1, -2, 0.75, 0.0625, -0.5]; let s = 0; for (let i = 0; i < n; i++) { let d = 0; for (let j = 0; j < 8; j++) d += w[j] * (((i + j) & 15) - 8); const z = d * 0.125; s += Math.floor((1 / (1 + Math.exp(-z))) * 1048576); } return s; };
ops.v_weakmap_get = function (n) { const keys = []; const wm = new WeakMap(); for (let j = 0; j < 16; j++) { const k = { id: j }; keys.push(k); wm.set(k, j * 3); } let s = 0; for (let i = 0; i < n; i++) { s = (s + wm.get(keys[i & 15])) | 0; } return s; };
globalThis.PRIMITIVE_OPS = ops;
