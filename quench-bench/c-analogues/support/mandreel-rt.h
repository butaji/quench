/* Runtime for the C back-translation of JetStream's Octane/mandreel.js (see tools/mandreel-to-c.cjs).
 *
 * The JS heap is one byte array of mandreel_total_memory bytes. Each typed-array view of the JS
 * (heap8, heapU8, heap16, heapU16, heap32, heapU32, heapFloat, heapDouble) is a load and a store
 * macro over the same bytes, with the JS conversions: loads widen to double, stores apply
 * ToInt8/ToInt16/ToInt32/ToUint32 (modular) or round to float32. The may_alias types let the
 * same bytes be read through several views. JS numbers are doubles; the JS_* helpers implement
 * the bitwise operators exactly (ToInt32 of any double, then the int32 operation).
 */
#ifndef MANDREEL_RT_H
#define MANDREEL_RT_H

#include <math.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>

#define JS_MEM 15908864 /* mandreel_total_memory */

typedef int8_t m_i8 __attribute__((may_alias));
typedef uint8_t m_u8 __attribute__((may_alias));
typedef int16_t m_i16 __attribute__((may_alias));
typedef uint16_t m_u16 __attribute__((may_alias));
typedef int32_t m_i32 __attribute__((may_alias));
typedef uint32_t m_u32 __attribute__((may_alias));
typedef float m_f32 __attribute__((may_alias));
typedef double m_f64 __attribute__((may_alias));

/* The live heap and the snapshot (my_heap). Declared without a size: every access is range-checked below. */
extern uint8_t js_heap[] __attribute__((aligned(8)));
extern uint8_t js_my_heap[] __attribute__((aligned(8)));
extern void *js_fntab[];                                       /* __FUNCTION_TABLE__ */

/* Element accesses. A JS typed array ignores a store to an index that is not an integer in range,
 * and a load from one gives undefined: NaN for the float views, 0 for the integer views (the generated
 * code uses those only where the JS value is an integer). Such an access is counted in js_oob_count,
 * which the harness requires to be zero, so the benchmark run never takes this path. Each access is
 * one range test on an int64 index; a double index is converted first (-1 when it is not an integer
 * in range), so it fails the same test. */
extern uint64_t js_oob_count;
static inline void js_oob(void);
__attribute__((noinline, cold)) void js_oob_slow(void);
static inline void js_oob(void) { js_oob_slow(); }
#define JS_LEN8 ((int64_t)JS_MEM)
#define JS_LEN16 ((int64_t)(JS_MEM / 2))
#define JS_LEN32 ((int64_t)(JS_MEM / 4))
#define JS_LEN64 ((int64_t)(JS_MEM / 8))
static inline __attribute__((always_inline)) int64_t js_dindex(double d, int64_t n) {
  if (d >= 0.0 && d < (double)n) {
    int64_t i = (int64_t)d;
    if ((double)i == d && i < n) return i;
  }
  return -1;
}
static inline __attribute__((always_inline)) int64_t js_iindex(int64_t i, int64_t n) { (void)n; return i; }
#define JS_IDX(i, n) _Generic((i), double: js_dindex, default: js_iindex)((i), (n))

/* Loads (the macro names the JS view; the function takes the element index). */
static inline __attribute__((always_inline)) int32_t js_li_i8(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN8) return (int32_t)((const m_i8 *)(js_heap))[i];
  js_oob();
  return 0;
}
#define LI_I8(i) js_li_i8(JS_IDX((i), JS_LEN8))
static inline __attribute__((always_inline)) int32_t js_li_u8(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN8) return (int32_t)((const m_u8 *)(js_heap))[i];
  js_oob();
  return 0;
}
#define LI_U8(i) js_li_u8(JS_IDX((i), JS_LEN8))
static inline __attribute__((always_inline)) int32_t js_li_i16(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN16) return (int32_t)((const m_i16 *)(js_heap))[i];
  js_oob();
  return 0;
}
#define LI_I16(i) js_li_i16(JS_IDX((i), JS_LEN16))
static inline __attribute__((always_inline)) int32_t js_li_u16(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN16) return (int32_t)((const m_u16 *)(js_heap))[i];
  js_oob();
  return 0;
}
#define LI_U16(i) js_li_u16(JS_IDX((i), JS_LEN16))
static inline __attribute__((always_inline)) int32_t js_li_i32(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN32) return (int32_t)((const m_i32 *)(js_heap))[i];
  js_oob();
  return 0;
}
#define LI_I32(i) js_li_i32(JS_IDX((i), JS_LEN32))
static inline __attribute__((always_inline)) int64_t js_ll_u32(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN32) return (int64_t)((const m_u32 *)(js_heap))[i];
  js_oob();
  return 0;
}
#define LL_U32(i) js_ll_u32(JS_IDX((i), JS_LEN32))
static inline __attribute__((always_inline)) double js_ld_f32(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN32) return (double)((const m_f32 *)(js_heap))[i];
  js_oob();
  return NAN;
}
#define LD_F32(i) js_ld_f32(JS_IDX((i), JS_LEN32))
static inline __attribute__((always_inline)) double js_ld_f64(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN64) return (double)((const m_f64 *)(js_heap))[i];
  js_oob();
  return NAN;
}
#define LD_F64(i) js_ld_f64(JS_IDX((i), JS_LEN64))
static inline __attribute__((always_inline)) int32_t js_li_mi8(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN8) return (int32_t)((const m_i8 *)(js_my_heap))[i];
  js_oob();
  return 0;
}
#define LI_MI8(i) js_li_mi8(JS_IDX((i), JS_LEN8))
static inline __attribute__((always_inline)) int32_t js_li_mi32(int64_t i) {
  if ((uint64_t)i < (uint64_t)JS_LEN32) return (int32_t)((const m_i32 *)(js_my_heap))[i];
  js_oob();
  return 0;
}
#define LI_MI32(i) js_li_mi32(JS_IDX((i), JS_LEN32))
#define LD_I8(i) ((double)LI_I8(i))
#define LD_U8(i) ((double)LI_U8(i))
#define LD_I16(i) ((double)LI_I16(i))
#define LD_U16(i) ((double)LI_U16(i))
#define LD_I32(i) ((double)LI_I32(i))
#define LD_U32(i) ((double)LL_U32(i))
#define LD_MI8(i) ((double)LI_MI8(i))
#define LD_MI32(i) ((double)LI_MI32(i))
/* Stores take the value already converted: int32 for the integer views (modular), double for floats. */
static inline __attribute__((always_inline)) void js_st_i8(int64_t i, int32_t v) {
  if ((uint64_t)i < (uint64_t)JS_LEN8) ((m_i8 *)(js_heap))[i] = (int8_t)v;
  else js_oob();
}
#define ST_I8(i, v) js_st_i8(JS_IDX((i), JS_LEN8), (v))
static inline __attribute__((always_inline)) void js_st_u8(int64_t i, int32_t v) {
  if ((uint64_t)i < (uint64_t)JS_LEN8) ((m_u8 *)(js_heap))[i] = (uint8_t)v;
  else js_oob();
}
#define ST_U8(i, v) js_st_u8(JS_IDX((i), JS_LEN8), (v))
static inline __attribute__((always_inline)) void js_st_i16(int64_t i, int32_t v) {
  if ((uint64_t)i < (uint64_t)JS_LEN16) ((m_i16 *)(js_heap))[i] = (int16_t)v;
  else js_oob();
}
#define ST_I16(i, v) js_st_i16(JS_IDX((i), JS_LEN16), (v))
static inline __attribute__((always_inline)) void js_st_u16(int64_t i, int32_t v) {
  if ((uint64_t)i < (uint64_t)JS_LEN16) ((m_u16 *)(js_heap))[i] = (uint16_t)v;
  else js_oob();
}
#define ST_U16(i, v) js_st_u16(JS_IDX((i), JS_LEN16), (v))
static inline __attribute__((always_inline)) void js_st_i32(int64_t i, int32_t v) {
  if ((uint64_t)i < (uint64_t)JS_LEN32) ((m_i32 *)(js_heap))[i] = v;
  else js_oob();
}
#define ST_I32(i, v) js_st_i32(JS_IDX((i), JS_LEN32), (v))
static inline __attribute__((always_inline)) void js_st_u32(int64_t i, int32_t v) {
  if ((uint64_t)i < (uint64_t)JS_LEN32) ((m_u32 *)(js_heap))[i] = (uint32_t)v;
  else js_oob();
}
#define ST_U32(i, v) js_st_u32(JS_IDX((i), JS_LEN32), (v))
static inline __attribute__((always_inline)) void js_st_f32(int64_t i, double v) {
  if ((uint64_t)i < (uint64_t)JS_LEN32) ((m_f32 *)(js_heap))[i] = (float)v;
  else js_oob();
}
#define ST_F32(i, v) js_st_f32(JS_IDX((i), JS_LEN32), (v))
static inline __attribute__((always_inline)) void js_st_f64(int64_t i, double v) {
  if ((uint64_t)i < (uint64_t)JS_LEN64) ((m_f64 *)(js_heap))[i] = v;
  else js_oob();
}
#define ST_F64(i, v) js_st_f64(JS_IDX((i), JS_LEN64), (v))
static inline __attribute__((always_inline)) void js_st_mi8(int64_t i, int32_t v) {
  if ((uint64_t)i < (uint64_t)JS_LEN8) ((m_i8 *)(js_my_heap))[i] = (int8_t)v;
  else js_oob();
}
#define ST_MI8(i, v) js_st_mi8(JS_IDX((i), JS_LEN8), (v))
static inline __attribute__((always_inline)) void js_st_mi32(int64_t i, int32_t v) {
  if ((uint64_t)i < (uint64_t)JS_LEN32) ((m_i32 *)(js_my_heap))[i] = v;
  else js_oob();
}
#define ST_MI32(i, v) js_st_mi32(JS_IDX((i), JS_LEN32), (v))

/* ToInt32: exact for every double (NaN and infinities give 0, others wrap modulo 2^32). */
static inline __attribute__((always_inline)) int32_t js_toint32_slow(double d) {
  if (!isfinite(d)) return 0;
  d = fmod(trunc(d), 4294967296.0);
  if (d < 0) d += 4294967296.0;
  return (int32_t)(uint32_t)d;
}
static inline __attribute__((always_inline)) int32_t js_toint32(double d) {
  if (d >= -2147483648.0 && d <= 2147483647.0) return (int32_t)d;
  return js_toint32_slow(d);
}
static inline __attribute__((always_inline)) uint32_t js_touint32(double d) { return (uint32_t)js_toint32(d); }

static inline __attribute__((always_inline)) double JS_TOI32(double a) { return (double)js_toint32(a); }
static inline __attribute__((always_inline)) double JS_OR(double a, double b) { return (double)(js_toint32(a) | js_toint32(b)); }
static inline __attribute__((always_inline)) double JS_AND(double a, double b) { return (double)(js_toint32(a) & js_toint32(b)); }
static inline __attribute__((always_inline)) double JS_XOR(double a, double b) { return (double)(js_toint32(a) ^ js_toint32(b)); }
static inline __attribute__((always_inline)) double JS_NOT(double a) { return (double)(~js_toint32(a)); }
static inline __attribute__((always_inline)) double JS_SHL(double a, double b) {
  return (double)(int32_t)((uint32_t)js_toint32(a) << (js_touint32(b) & 31u));
}
static inline __attribute__((always_inline)) double JS_SAR(double a, double b) { return (double)(js_toint32(a) >> (js_touint32(b) & 31u)); }
static inline __attribute__((always_inline)) double JS_SHR(double a, double b) { return (double)((uint32_t)js_toint32(a) >> (js_touint32(b) & 31u)); }
static inline __attribute__((always_inline)) double JS_IMUL(double a, double b) {
  return (double)(int32_t)((uint32_t)js_toint32(a) * (uint32_t)js_toint32(b));
}

/* JS uint(v) of a double (the translator inlines it for integer arguments). */
static inline __attribute__((always_inline)) double js_uint(double v) { return v >= 0 ? v : 4294967296.0 + v; }

/* ToBoolean of a number. */
static inline __attribute__((always_inline)) int JS_TRUTHY(double x) { return x != 0.0 && x == x; }

/* The key a switch statement matches case labels against (NaN and non-integers match none). */
static inline __attribute__((always_inline)) int64_t js_switch_key(double d) {
  if (d >= -9007199254740992.0 && d <= 9007199254740992.0 && d == (double)(int64_t)d) return (int64_t)d;
  return INT64_MIN;
}

/* Math functions with JS semantics where C differs. */
static inline __attribute__((always_inline)) double JS_FROUND(double x) { return (double)(float)x; }
static inline __attribute__((always_inline)) double JS_SIGN(double x) { return x > 0 ? 1.0 : x < 0 ? -1.0 : x; }
static inline __attribute__((always_inline)) double JS_ROUND(double x) {
  if (!isfinite(x) || x == 0) return x;
  if (x > 0 && x < 0.5) return 0.0;
  if (x < 0 && x >= -0.5) return -0.0;
  double r = floor(x);
  return (x - r >= 0.5) ? r + 1.0 : r;
}
static inline __attribute__((always_inline)) double JS_POW(double x, double y) {
  if (y != y) return NAN;
  if (y == 0) return 1.0;
  if ((x == 1.0 || x == -1.0) && !isfinite(y)) return NAN;
  return pow(x, y);
}
static inline __attribute__((always_inline)) double JS_MIN(double a, double b) {
  if (a != a || b != b) return NAN;
  if (a == 0 && b == 0) return signbit(a) ? a : b;
  return a < b ? a : b;
}
static inline __attribute__((always_inline)) double JS_MAX(double a, double b) {
  if (a != a || b != b) return NAN;
  if (a == 0 && b == 0) return signbit(a) ? b : a;
  return a > b ? a : b;
}

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif
#ifndef M_E
#define M_E 2.7182818284590452354
#endif
#ifndef M_LN2
#define M_LN2 0.69314718055994530942
#endif
#ifndef M_LN10
#define M_LN10 2.30258509299404568402
#endif
#ifndef M_SQRT2
#define M_SQRT2 1.4142135623730951
#endif
#ifndef M_SQRT1_2
#define M_SQRT1_2 0.70710678118654752440
#endif

/* A numeric JS global, for the state check (name as in the JS source). */
struct js_global_entry {
  const char *name;
  double *value;
};
extern const struct js_global_entry js_global_table[];

/* Runtime calls made by the generated code (defined in mandreel.c). */
/* A run of data emits with literal operands (see FnGen.emitRun in tools/mandreel-to-c.cjs): count
 * stores of width bytes (1, 2 or 4) of data, each at the emit address, which advances by width. */
struct js_emit_op {
  int32_t data;
  int32_t count;
  int32_t width;
};
void js_emit_ops(const struct js_emit_op *ops, int n);
double js_register_delegate(void *fn);
double js_emit_bytes(const uint16_t *units, int count);
double js_fill_cstring(double ptr, const char *s);
double js_perf_now(void);
void js_unsupported(const char *function, const char *what);
void js_unsupported_at(const char *function, const char *what);

#endif
