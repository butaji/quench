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
globalThis.PRIMITIVE_OPS = ops;
