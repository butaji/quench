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
globalThis.PRIMITIVE_OPS = ops;
