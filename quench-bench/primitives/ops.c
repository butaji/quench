/*
 * C twins of quench-bench/primitives/ops.js (the per-primitive instruction budget
 * in crates/quench-tier-corpus/ir_budget.txt quotes their Ir per iteration).
 *
 * Typing rule (JS number -> C type, decided per variable from its assignments):
 *   int32_t : loop counters, and variables whose every assignment is a
 *             bitwise/|0 result, an int32 variable, or an int literal.
 *             Wrapping arithmetic goes through uint32_t so there is no UB.
 *   uint32_t: only for the >>> result and the operands of |0 wrap.
 *   double  : everything else (counters/accumulators fed by + - * / on
 *             non-|0 values, Math.* results, fractional values).
 *   Arrays  : int ops -> int32_t[256]; double -> double[256];
 *             objects -> struct obj * [256] (malloc'd); typed arrays -> matching C arrays.
 *   Strings : const char * + length. charCodeAt = byte load; str_concat = snprintf;
 *             str_concat_acc = growable buffer; num_to_str = snprintf length;
 *             str_to_num = strtod; parse_int = strtol(base 10); eq_str = len + memcmp;
 *             str_compare = strcmp.
 *
 * Every value that JS would read from a variable/literal the engine cannot fold is
 * laundered through an empty asm so gcc cannot constant-fold the op away.
 * Every measured loop body starts with BODY_BARRIER() (empty volatile asm): this
 * blocks deletion, closed-form replacement and vectorization and emits no instructions.
 *
 * Build: cc -O2 -fno-tree-vectorize -fno-unroll-loops -o ops_c ops.c -lm
 * Run:   ./ops_c <opname> <n>   prints "<opname> <result>"
 */
#define _GNU_SOURCE /* memmem, for the rx_lit twin */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#define NOINL __attribute__((noinline))
#define BODY_BARRIER() __asm__ volatile("")
#define OPAQUE_I(v) __asm__ volatile("" : "+r"(v))
#define OPAQUE_D(v) __asm__ volatile("" : "+x"(v))
#define OPAQUE_P(v) __asm__ volatile("" : "+r"(v))
#define ESCAPE(p) __asm__ volatile("" : : "r"(p) : "memory")

struct obj { int32_t v; };

struct ivec { int32_t *d; size_t len, cap; };

static const char S[] = "the quick brown fox jumps over the lazy dog 0123456789";

/* ---------------- int / bitwise ---------------- */

static double NOINL op_empty(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = i; }
  return a;
}

static double NOINL op_int_add(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)i); }
  return a;
}

static double NOINL op_int_add_noor(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = a + (double)(i & 7); }
  return a;
}

static double NOINL op_int_sub(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a - (uint32_t)i); }
  return a;
}

static double NOINL op_int_mul(long n) {
  int32_t N = (int32_t)n, a = 1;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a * 3u + (uint32_t)i); }
  return a;
}

static double NOINL op_int_mul_ovf(long n) {
  int32_t N = (int32_t)n;
  double a = 1;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = fmod(a * 65537.0, 1000003.0); }
  return a;
}

static double NOINL op_int_div(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 1; i < N; i++) {
    BODY_BARRIER();
    /* JS: (1000003 / i) | 0 is an exact truncating quotient for positive operands. */
    a = (int32_t)((uint32_t)a + (uint32_t)(1000003 / i));
  }
  return a;
}

static double NOINL op_int_mod(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 1; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)(i % 7)); }
  return a;
}

static double NOINL op_int_mod_var(long n) {
  int32_t N = (int32_t)n, a = 0, m = 13;
  OPAQUE_I(m); /* a JS `let` binding the engine does not fold */
  for (int32_t i = 1; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)(i % m)); }
  return a;
}

static double NOINL op_shl_shr(long n) {
  int32_t N = (int32_t)n, a = 1;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = ((int32_t)((uint32_t)a << 3)) ^ (a >> 2) ^ i;
  }
  return a;
}

static double NOINL op_ushr(long n) {
  int32_t N = (int32_t)n, a = -1;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)(((uint32_t)a >> 1) + (uint32_t)i);
  }
  return a;
}

static double NOINL op_bitwise(long n) {
  int32_t N = (int32_t)n, a = 0x12345;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = ((a & 0xff00ff) | (i ^ 0x55)) & 0xfffffff;
  }
  return a;
}

static double NOINL op_to_uint32(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    uint32_t u = (uint32_t)i * 2654435761u; /* JS: (i*2654435761) >>> 0, exact mod 2^32 */
    a = fmod(a + (double)u, 4294967296.0);
  }
  return a;
}

/* ---------------- double ---------------- */

static double NOINL op_dbl_add(long n) {
  int32_t N = (int32_t)n;
  double a = 0.5;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = a + 0.25; }
  return a;
}

static double NOINL op_dbl_mul(long n) {
  int32_t N = (int32_t)n;
  double a = 1.0000001;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = a * 1.0000001; }
  return a;
}

static double NOINL op_dbl_div(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 1; i < N; i++) { BODY_BARRIER(); a = a + 1.0 / (double)i; }
  return a;
}

static double NOINL op_mixed_int_dbl(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = a + (double)i * 0.5; }
  return a;
}

/* ---------------- comparisons / counters (counters are double, per typing rule) ---------------- */

static double NOINL op_cmp_int(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (i < 1000 || (i & 3) > 1) c = c + 1.0;
  }
  return c;
}

static double NOINL op_cmp_dbl(long n) {
  int32_t N = (int32_t)n;
  double c = 0, x = 0.5;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    x += 0.001;
    if (x < 100.5) c = c + 1.0;
  }
  return c;
}

static double NOINL op_eq_int(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if ((i & 7) == 3) c = c + 1.0;
  }
  return c;
}

static double NOINL op_eq_str(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *a = "abc", *b = "abd", *d = "abc";
  int32_t la = 3, lb = 3, ld = 3;
  OPAQUE_P(a); OPAQUE_P(b); OPAQUE_P(d);
  OPAQUE_I(la); OPAQUE_I(lb); OPAQUE_I(ld);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (la == ld && memcmp(a, d, (size_t)la) == 0) c = c + 1.0;
    if (la == lb && memcmp(a, b, (size_t)la) == 0) c = c - 1.0;
  }
  return c;
}

static double NOINL op_eq_undef_null(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const void *u = NULL; /* JS undefined modelled as a null pointer; null compares equal too */
  OPAQUE_P(u);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (u == NULL) c = c + 1.0; /* u === undefined */
    if (u == NULL) c = c + 1.0; /* u == null */
  }
  return c;
}

static double NOINL op_truthy_int(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (i & 3) c = c + 1.0; }
  return c;
}

static double NOINL op_truthy_obj(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  struct obj *o = malloc(sizeof *o);
  o->v = 1;
  OPAQUE_P(o);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (o != NULL) c = c + 1.0; }
  free(o);
  return c;
}

static double NOINL op_truthy_str(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *s = "x";
  size_t slen = 1;
  OPAQUE_P(s); OPAQUE_P(slen);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (slen != 0) c = c + 1.0; }
  return c;
}

static double NOINL op_not_op(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (!(i & 1)) c = c + 1.0; }
  return c;
}

static double NOINL op_neg_op(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)0 - (uint32_t)a + (uint32_t)i);
  }
  return a;
}

static double NOINL op_typeof_op(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  int32_t is_number = 1; /* typeof of a statically-number value: the test is a constant true */
  OPAQUE_I(is_number);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (is_number) c = c + 1.0; }
  return c;
}

static double NOINL op_inc_local(long n) {
  int32_t N = (int32_t)n;
  double a = 0, b = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = a + 1.0; b = b - 1.0; }
  return a + b + 1.0;
}

/* ---------------- Math ---------------- */

static double NOINL op_math_floor(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a += floor((double)i * 0.37); }
  return a;
}

static double NOINL op_math_sqrt(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a += sqrt((double)i); }
  return a;
}

static double NOINL op_math_abs(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a += fabs((double)i - 500.0); }
  return a;
}

static double NOINL op_math_minmax(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t mn = i < 777 ? i : 777;
    int32_t mx = i > 5 ? i : 5;
    a += (double)mn + (double)mx;
  }
  return a;
}

static double NOINL op_math_imul(long n) {
  int32_t N = (int32_t)n, a = 1;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a * 31u + (uint32_t)i); /* Math.imul(a,31) + i | 0 */
  }
  return a;
}

static double NOINL op_math_round(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a += floor((double)i * 0.37 + 0.5); }
  return a;
}

/* ---------------- number <-> string ---------------- */

static double NOINL op_num_to_str(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  char buf[32];
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int len = snprintf(buf, sizeof buf, "%d", (int)i);
    a += (double)len;
  }
  return a;
}

static double NOINL op_str_to_num(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *s = "12345";
  OPAQUE_P(s);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)(int32_t)strtod(s, NULL));
  }
  return a;
}

static double NOINL op_parse_int(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *s = "12345";
  OPAQUE_P(s);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)strtol(s, NULL, 10));
  }
  return a;
}

/* ---------------- strings ---------------- */

static double NOINL op_str_length(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  const char *s = S;
  size_t slen = strlen(s);
  OPAQUE_P(s); OPAQUE_P(slen);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a += (double)slen; }
  return a;
}

static double NOINL op_str_charcodeat(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *s = S;
  int32_t l = (int32_t)strlen(s);
  OPAQUE_P(s); OPAQUE_I(l);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)(uint8_t)s[i % l]);
  }
  return a;
}

static double NOINL op_str_index(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  const char *s = S;
  int32_t l = (int32_t)strlen(s);
  OPAQUE_P(s); OPAQUE_I(l);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (s[i % l] == 'o') a = a + 1.0;
  }
  return a;
}

static double NOINL op_str_concat(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  char buf[32];
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int len = snprintf(buf, sizeof buf, "ab%d", (int)(i & 15));
    a += (double)len;
  }
  return a;
}

static double NOINL op_str_concat_acc(long n) {
  int32_t N = (int32_t)n;
  size_t len = 0, cap = 16;
  char *buf = malloc(cap);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (len + 1 > cap) {
      cap *= 2;
      buf = realloc(buf, cap);
    }
    buf[len++] = 'x';
  }
  double r = (double)len;
  free(buf);
  return r;
}

static double NOINL op_str_compare(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *a = "apple", *b = "apricot";
  OPAQUE_P(a); OPAQUE_P(b);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (strcmp(a, b) < 0) c = c + 1.0; }
  return c;
}

/* ---------------- arrays ---------------- */

static double NOINL op_arr_load_int(long n) {
  int32_t N = (int32_t)n, a = 0;
  int32_t arr[256];
  for (int32_t i = 0; i < 256; i++) arr[i] = i;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)arr[i & 255]);
  }
  return a;
}

static double NOINL op_arr_store_int(long n) {
  int32_t N = (int32_t)n;
  int32_t arr[256];
  for (int32_t i = 0; i < 256; i++) arr[i] = 0;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); arr[i & 255] = i; }
  return arr[7];
}

static double NOINL op_arr_load_dbl(long n) {
  int32_t N = (int32_t)n;
  double arr[256], a = 0;
  for (int32_t i = 0; i < 256; i++) arr[i] = (double)i + 0.5;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a += arr[i & 255]; }
  return a;
}

static double NOINL op_arr_store_dbl(long n) {
  int32_t N = (int32_t)n;
  double arr[256];
  for (int32_t i = 0; i < 256; i++) arr[i] = 0.5;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); arr[i & 255] = (double)i * 0.5; }
  return arr[7];
}

static double NOINL op_arr_load_obj(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct obj *arr[256];
  for (int32_t i = 0; i < 256; i++) { arr[i] = malloc(sizeof **arr); arr[i]->v = i; }
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)arr[i & 255]->v);
  }
  for (int32_t i = 0; i < 256; i++) free(arr[i]);
  return a;
}

static double NOINL op_arr_store_obj(long n) {
  int32_t N = (int32_t)n;
  struct obj *arr[256];
  struct obj *o = malloc(sizeof *o);
  o->v = 1;
  for (int32_t i = 0; i < 256; i++) arr[i] = o;
  ESCAPE(arr);
  OPAQUE_P(o);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); arr[i & 255] = o; }
  size_t len = 256;
  OPAQUE_P(len);
  free(o);
  return (double)len; /* arr.length */
}

static double NOINL op_arr_length(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  int32_t arr[5] = {1, 2, 3, 4, 5};
  int32_t alen = (int32_t)(sizeof arr / sizeof arr[0]);
  ESCAPE(arr);
  OPAQUE_I(alen);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a += (double)alen; }
  return a;
}

static double NOINL op_arr_push(long n) {
  int32_t N = (int32_t)n;
  struct ivec v = { NULL, 0, 0 };
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (v.len == v.cap) {
      v.cap = v.cap ? v.cap * 2 : 4;
      v.d = realloc(v.d, v.cap * sizeof *v.d);
    }
    v.d[v.len++] = i;
  }
  double r = (double)v.len;
  free(v.d);
  return r;
}

static double NOINL op_typed_load(long n) {
  int32_t N = (int32_t)n, a = 0;
  int32_t t[256];
  for (int32_t i = 0; i < 256; i++) t[i] = i;
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)t[i & 255]);
  }
  return a;
}

static double NOINL op_typed_store_f64(long n) {
  int32_t N = (int32_t)n;
  double t[256];
  for (int32_t i = 0; i < 256; i++) t[i] = 0;
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); t[i & 255] = (double)i * 0.5; }
  return t[7];
}

/* ---------------- driver ---------------- */

struct pair { int32_t x, y; };
static int32_t NOINL add_one(int32_t x) { return (int32_t)((uint32_t)x + 1u); }
struct env { int32_t k; };
static int32_t NOINL add_k(const struct env *e, int32_t x) { return (int32_t)((uint32_t)x + (uint32_t)e->k); }

static double NOINL op_field_read(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct pair o = {3, 4}, *p = &o;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)p->y); }
  return a;
}

static double NOINL op_field_store(long n) {
  int32_t N = (int32_t)n;
  struct pair o = {0, 0}, *p = &o;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); p->y = i; }
  return p->y;
}

static double NOINL op_call_direct(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)add_one(i)); }
  return a;
}

static double NOINL op_call_closure(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct env e = {5}, *p = &e;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)add_k(p, i)); }
  return a;
}

static double NOINL op_call_method(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct env e = {1}, *p = &e;
  int32_t (*volatile m)(const struct env *, int32_t) = add_k;
  int32_t (*f)(const struct env *, int32_t) = m;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)f(p, i)); }
  return a;
}


/* RegExp twins: RegExp.prototype.test of each ops.js rx_ pattern over RS, as the matcher a C
 * programmer writes for the pattern (same answer, same subject). */
static const char RS[] = "The quick brown fox jumps over the lazy dog; contact: john.doe+tag@example.com, price $1234.56, date 2026-11-18, running jumping";
static int is_word(unsigned char c) { return (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '_'; }
static int is_space(unsigned char c) { return c == ' ' || (c >= '\t' && c <= '\r'); }
static int NOINL rx_lit(const char *s, size_t len) { return memmem(s, len, "lazy", 4) != NULL; }
static int NOINL rx_digits(const char *s, size_t len) {
  for (size_t i = 0; i < len; i++) if (s[i] >= '0' && s[i] <= '9') return 1;
  return 0;
}
static int NOINL rx_word2(const char *s, size_t len) {
  /* (\w+)\s(\w+): a word unit, then whitespace, then a word unit. */
  for (size_t i = 1; i + 1 < len; i++)
    if (is_space((unsigned char)s[i]) && is_word((unsigned char)s[i - 1]) && is_word((unsigned char)s[i + 1])) return 1;
  return 0;
}
static int email_left(unsigned char c) { return is_word(c) || c == '.' || c == '+' || c == '-'; }
static int NOINL rx_email(const char *s, size_t len) {
  /* [\w.+-]+@[\w-]+\.[\w.]+ */
  for (const char *at = memchr(s, '@', len); at; at = memchr(at + 1, '@', len - (size_t)(at + 1 - s))) {
    if (at == s || !email_left((unsigned char)at[-1])) continue;
    size_t j = (size_t)(at - s) + 1, k = j;
    while (k < len && (is_word((unsigned char)s[k]) || s[k] == '-')) k++;
    if (k > j && k + 1 < len && s[k] == '.' && (is_word((unsigned char)s[k + 1]) || s[k + 1] == '.')) return 1;
  }
  return 0;
}
static int NOINL rx_dotstar(const char *s, size_t len) {
  /* q.*z: a q, then a z before the next line terminator. */
  for (const char *q = memchr(s, 'q', len); q; q = memchr(q + 1, 'q', len - (size_t)(q + 1 - s))) {
    for (const char *p = q + 1; p < s + len && *p != '\n' && *p != '\r'; p++) if (*p == 'z') return 1;
  }
  return 0;
}
static int NOINL rx_ahead(const char *s, size_t len) {
  /* \w+(?=@): an @ right after a word unit. */
  for (const char *at = memchr(s, '@', len); at; at = memchr(at + 1, '@', len - (size_t)(at + 1 - s)))
    if (at > s && is_word((unsigned char)at[-1])) return 1;
  return 0;
}
#define RX_OP(name) \
  static double NOINL op_##name(long n) { \
    int32_t N = (int32_t)n, c = 0; const char *s = RS; size_t len = sizeof RS - 1; \
    for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); OPAQUE_P(s); if (name(s, len)) c++; } \
    return c; \
  }
RX_OP(rx_lit) RX_OP(rx_digits) RX_OP(rx_word2) RX_OP(rx_email) RX_OP(rx_dotstar) RX_OP(rx_ahead)

typedef double (*opfn)(long);
static const struct { const char *name; opfn fn; } TABLE[] = {
  {"field_read", op_field_read}, {"field_store", op_field_store}, {"call_direct", op_call_direct},
  {"call_closure", op_call_closure}, {"call_method", op_call_method},
  {"empty", op_empty}, {"int_add", op_int_add}, {"int_add_noor", op_int_add_noor},
  {"int_sub", op_int_sub}, {"int_mul", op_int_mul}, {"int_mul_ovf", op_int_mul_ovf},
  {"int_div", op_int_div}, {"int_mod", op_int_mod}, {"int_mod_var", op_int_mod_var},
  {"shl_shr", op_shl_shr}, {"ushr", op_ushr}, {"bitwise", op_bitwise},
  {"to_uint32", op_to_uint32}, {"dbl_add", op_dbl_add}, {"dbl_mul", op_dbl_mul},
  {"dbl_div", op_dbl_div}, {"mixed_int_dbl", op_mixed_int_dbl}, {"cmp_int", op_cmp_int},
  {"cmp_dbl", op_cmp_dbl}, {"eq_int", op_eq_int}, {"eq_str", op_eq_str},
  {"eq_undef_null", op_eq_undef_null}, {"truthy_int", op_truthy_int},
  {"truthy_obj", op_truthy_obj}, {"truthy_str", op_truthy_str}, {"not_op", op_not_op},
  {"neg_op", op_neg_op}, {"typeof_op", op_typeof_op}, {"inc_local", op_inc_local},
  {"math_floor", op_math_floor}, {"math_sqrt", op_math_sqrt}, {"math_abs", op_math_abs},
  {"math_minmax", op_math_minmax}, {"math_imul", op_math_imul}, {"math_round", op_math_round},
  {"num_to_str", op_num_to_str}, {"str_to_num", op_str_to_num}, {"parse_int", op_parse_int},
  {"str_length", op_str_length}, {"str_charcodeat", op_str_charcodeat},
  {"str_index", op_str_index}, {"str_concat", op_str_concat},
  {"str_concat_acc", op_str_concat_acc}, {"str_compare", op_str_compare},
  {"arr_load_int", op_arr_load_int}, {"arr_store_int", op_arr_store_int},
  {"arr_load_dbl", op_arr_load_dbl}, {"arr_store_dbl", op_arr_store_dbl},
  {"arr_load_obj", op_arr_load_obj}, {"arr_store_obj", op_arr_store_obj},
  {"arr_length", op_arr_length}, {"arr_push", op_arr_push},
  {"typed_load", op_typed_load}, {"typed_store_f64", op_typed_store_f64},
  {"rx_lit", op_rx_lit}, {"rx_digits", op_rx_digits}, {"rx_word2", op_rx_word2},
  {"rx_email", op_rx_email}, {"rx_dotstar", op_rx_dotstar}, {"rx_ahead", op_rx_ahead},
};

int main(int argc, char **argv) {
  if (argc < 3) {
    fprintf(stderr, "usage: %s <op> <n>\n", argv[0]);
    return 2;
  }
  for (size_t k = 0; k < sizeof TABLE / sizeof TABLE[0]; k++) {
    if (strcmp(TABLE[k].name, argv[1]) == 0) {
      double r = TABLE[k].fn(atol(argv[2]));
      printf("%s %.17g\n", argv[1], r);
      return 0;
    }
  }
  fprintf(stderr, "unknown op %s\n", argv[1]);
  return 1;
}
