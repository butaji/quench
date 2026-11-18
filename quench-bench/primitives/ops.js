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
function addOne(x) { return (x + 1) | 0; }
// The object arrives as a parameter, so the front end cannot fold `o.y` to a constant.
function fieldRead(n, o) { let a = 0; for (let i = 0; i < n; i++) { a = (a + o.y) | 0; } return a; }
ops.field_read = function (n) { return fieldRead(n, { x: 3, y: 4 }); };
ops.field_store = function (n) { const o = { x: 0, y: 0 }; for (let i = 0; i < n; i++) { o.y = i; } return o.y; };
ops.call_direct = function (n) { let a = 0; for (let i = 0; i < n; i++) { a = (a + addOne(i)) | 0; } return a; };
ops.call_closure = function (n) { const k = 5, f = (x) => (x + k) | 0; let a = 0; for (let i = 0; i < n; i++) { a = (a + f(i)) | 0; } return a; };
ops.call_method = function (n) { const o = { k: 1, m(x) { return (x + this.k) | 0; } }; let a = 0; for (let i = 0; i < n; i++) { a = (a + o.m(i)) | 0; } return a; };
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
globalThis.PRIMITIVE_OPS = ops;
