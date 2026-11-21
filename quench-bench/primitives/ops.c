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
#include <setjmp.h>

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

static double NOINL op_str_includes(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  const char *s = S;
  OPAQUE_P(s);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const char *h = s; OPAQUE_P(h);
    if (strstr(h, "lazy")) a = a + 1.0;
  }
  return a;
}

static double NOINL op_str_slice(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  const char *s = S;
  OPAQUE_P(s);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    /* A new five-character string: allocated, copied, its length read. */
    char *buf = malloc(6);
    memcpy(buf, s + 4, 5); buf[5] = 0;
    OPAQUE_P(buf);
    a = a + (double)strlen(buf);
    free(buf);
  }
  return a;
}

static double NOINL op_arr_isarray(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  int32_t arr[3] = {1, 2, 3};
  const int32_t *p = arr;
  int32_t kind = 1;
  OPAQUE_P(p); OPAQUE_I(kind);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (kind == 1) a = a + 1.0;
  }
  return a;
}

static double NOINL op_arr_slice(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  int32_t arr[4] = {1, 2, 3, 4};
  int32_t *p = arr;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    /* A new two-element array: allocated, filled from the source, its length read. */
    int32_t *out = malloc(2 * sizeof *out);
    memcpy(out, p, 2 * sizeof *out);
    OPAQUE_P(out);
    a = a + 2.0;
    free(out);
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

static double NOINL op_typed_store_u8c(long n) {
  int32_t N = (int32_t)n;
  uint8_t t[256];
  for (int32_t i = 0; i < 256; i++) t[i] = 0;
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    double v = (double)(i & 511) * 0.75 - 64.0;
    t[i & 255] = v > 0.0 ? (v < 255.0 ? (uint8_t)nearbyint(v) : 255) : 0;
  }
  return (double)t[7] + (double)t[200];
}

/* ---------------- driver ---------------- */

struct pair { int32_t x, y; };
static int32_t NOINL add_one(int32_t x) { return (int32_t)((uint32_t)x + 1u); }
struct env { int32_t k; };
static int32_t NOINL add_k(const struct env *e, int32_t x) { return (int32_t)((uint32_t)x + (uint32_t)e->k); }
/* A variadic function's rest values as C passes them: a count and the values in place. */
static int32_t NOINL sum_rest(int32_t count, const int32_t *xs) { return count > 1 ? (int32_t)((uint32_t)xs[0] + (uint32_t)xs[1]) : 0; }

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

static double NOINL op_call_rest(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); int32_t xs[2] = {i, 1}; a = (int32_t)((uint32_t)a + (uint32_t)sum_rest(2, xs)); }
  return a;
}

static double NOINL op_call_spread(long n) {
  int32_t N = (int32_t)n, a = 0, xs[2] = {0, 1};
  int32_t *p = xs;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); p[0] = i; a = (int32_t)((uint32_t)a + (uint32_t)sum_rest(2, p)); }
  return a;
}

static int32_t NOINL forward_rest(int32_t count, const int32_t *xs) { return sum_rest(count, xs); }

static double NOINL op_call_rest_forward(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); int32_t xs[2] = {i, 1}; a = (int32_t)((uint32_t)a + (uint32_t)forward_rest(2, xs)); }
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
/* ---------------- kernels ---------------- */

struct body { double x, y, z, vx, vy, vz, mass; };

static double NOINL op_k_nbody_pair(long n) {
  int32_t N = (int32_t)n;
  struct body A = {0, 0, 0, 0, 0, 0, 39.47}, B = {4.84, -1.16, -0.1, 0.6, 2.81, -0.02, 0.037};
  struct body *a = &A, *b = &B;
  OPAQUE_P(a); OPAQUE_P(b);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    double dx = a->x - b->x, dy = a->y - b->y, dz = a->z - b->z;
    double d2 = dx * dx + dy * dy + dz * dz;
    double mag = 0.01 / (d2 * sqrt(d2));
    double bm = b->mass * mag, am = a->mass * mag;
    a->vx -= dx * bm; a->vy -= dy * bm; a->vz -= dz * bm;
    b->vx += dx * am; b->vy += dy * am; b->vz += dz * am;
  }
  return a->vx + b->vz;
}

struct point { int32_t x, y; };

static double NOINL op_k_struct_sum(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct point *pts = malloc(64 * sizeof *pts);
  for (int32_t i = 0; i < 64; i++) { pts[i].x = i; pts[i].y = i + 1; }
  ESCAPE(pts);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct point *p = &pts[i & 63];
    s = (int32_t)((uint32_t)s + (uint32_t)p->x * (uint32_t)p->y);
  }
  free(pts);
  return s;
}

struct dpoint { double x, y; };

static double NOINL op_k_dist2(long n) {
  int32_t N = (int32_t)n; double s = 0;
  struct dpoint *pts = malloc(64 * sizeof *pts);
  for (int32_t i = 0; i < 64; i++) { pts[i].x = i + 0.5; pts[i].y = i + 0.25; }
  ESCAPE(pts);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct dpoint *p = &pts[i & 63];
    s += p->x * p->x + p->y * p->y;
  }
  free(pts);
  return s;
}

static double NOINL op_k_field_acc(long n) {
  int32_t N = (int32_t)n;
  struct dpoint *p = malloc(sizeof *p);
  p->x = 0.5; p->y = 0.25;
  ESCAPE(p);
  double dx = 0.125;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); p->x += dx; }
  double r = p->x + p->y;
  free(p);
  return r;
}

struct node { int32_t val; struct node *next; };

static double NOINL op_k_list_walk(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct node *nodes = malloc(64 * sizeof *nodes), *head = 0;
  for (int32_t i = 63; i >= 0; i--) { nodes[i].val = i; nodes[i].next = head; head = &nodes[i]; }
  ESCAPE(nodes);
  struct node *node = head;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)((uint32_t)s + (uint32_t)node->val);
    node = node->next;
    if (!node) node = head;
  }
  free(nodes);
  return s;
}

static double NOINL op_k_vec_local(long n) {
  int32_t N = (int32_t)n; double s = 0;
  struct dpoint a = {1.5, 2.5}, b = {0.25, 0.75};
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct dpoint c = {a.x + i, a.y + b.y};
    s += c.x * b.x + c.y;
  }
  return s;
}

struct state5 { int32_t a, b, c, d, n; };

static double NOINL op_obj_spread(long n) {
  int32_t N = (int32_t)n;
  struct state5 *state = malloc(sizeof *state);
  *state = (struct state5){1, 2, 3, 4, 0};
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct state5 *next = malloc(sizeof *next);
    *next = *state;
    next->n = i;
    ESCAPE(next);
    free(state);
    state = next;
  }
  double r = state->n + state->a;
  free(state);
  return r;
}

struct triple { int32_t x, y, z; };

static double NOINL op_alloc_literal(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct point *p = malloc(sizeof *p);
    p->x = i; p->y = 1;
    ESCAPE(p);
    s = (int32_t)((uint32_t)s + (uint32_t)p->x + (uint32_t)p->y);
    free(p);
  }
  return s;
}

struct cell1 { int32_t v; };

static double NOINL op_alloc_literal1(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct cell1 *p = malloc(sizeof *p);
    p->v = i;
    ESCAPE(p);
    s = (int32_t)((uint32_t)s + (uint32_t)p->v);
    free(p);
  }
  return s;
}

static double NOINL op_alloc_ctor(long n) { return op_alloc_literal(n); }

static double NOINL op_alloc_class(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct triple *p = malloc(sizeof *p);
    p->x = i; p->y = 1; p->z = 2;
    ESCAPE(p);
    s = (int32_t)((uint32_t)s + (uint32_t)p->x + (uint32_t)p->z);
    free(p);
  }
  return s;
}

struct task { int32_t (*run)(const struct task *, int32_t); int32_t k; };
static int32_t NOINL task_a(const struct task *t, int32_t x) { return (int32_t)((uint32_t)x + (uint32_t)t->k); }
static int32_t NOINL task_b(const struct task *t, int32_t x) { return x ^ t->k; }

static double NOINL op_k_dispatch(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct task tasks[4] = {{task_a, 3}, {task_b, 5}, {task_a, 7}, {task_b, 9}};
  struct task *t = tasks;
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct task *k = &t[i & 3];
    a = k->run(k, (int32_t)((uint32_t)a + (uint32_t)i));
  }
  return a;
}

static double NOINL op_k_json(long n) {
  int32_t N = (int32_t)n, a = 0;
  char buf[64];
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int len = snprintf(buf, sizeof buf, "{\"id\":%d,\"name\":\"ab\",\"ok\":true}", i & 1023);
    ESCAPE(buf);
    const char *id = strstr(buf, "\"id\":");
    int32_t v = (int32_t)strtol(id + 5, NULL, 10);
    a = (int32_t)((uint32_t)a + (uint32_t)v + (uint32_t)len);
  }
  return a;
}

/* /o+x/ searched as a compiled matcher would: an 'o', more 'o's, then 'x'. */
static int NOINL match_o_plus_x(const char *s, size_t len) {
  for (size_t start = 0; start < len; start++) {
    if (s[start] != 'o') continue;
    size_t k = start + 1;
    while (k < len && s[k] == 'o') k++;
    if (k < len && s[k] == 'x') return 1;
  }
  return 0;
}

static double NOINL op_k_regexp(long n) {
  int32_t N = (int32_t)n, a = 0;
  static const char *words[4] = {"foox", "bar", "ox", "zoooox"};
  static const size_t lens[4] = {4, 3, 2, 6};
  const char *const *w = words;
  OPAQUE_P(w);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (match_o_plus_x(w[i & 3], lens[i & 3])) a++;
  }
  return a;
}

static double NOINL op_k_str_build(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  char *s = malloc(256);
  size_t len = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s[len++] = 'k';
    s[len++] = (char)('0' + (i & 7));
    ESCAPE(s);
    if ((i & 63) == 63) { a += (double)len; len = 0; }
  }
  free(s);
  return a + (double)len;
}

/* ---------------- builtins that run no user code ---------------- */
#include <time.h>
static double NOINL op_date_now(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct timespec t; clock_gettime(CLOCK_REALTIME, &t);
    double ms = (double)t.tv_sec * 1000.0 + (double)(t.tv_nsec / 1000000);
    a = (int32_t)((uint32_t)a + ((uint32_t)(int64_t)ms & 1u));
  }
  return a > -1 ? 1 : 0;
}
static int32_t kv_find(const int32_t *arr, int32_t len, int32_t key) { for (int32_t j = 0; j < len; j++) if (arr[j] == key) return j; return -1; }
static double NOINL op_arr_includes(long n) {
  int32_t N = (int32_t)n, a = 0, arr[64];
  for (int32_t j = 0; j < 64; j++) arr[j] = j * 2;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (kv_find(arr, 64, i & 127) >= 0) a++; }
  return a;
}
static double NOINL op_arr_indexof(long n) {
  int32_t N = (int32_t)n, a = 0, arr[64];
  for (int32_t j = 0; j < 64; j++) arr[j] = j * 2;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)kv_find(arr, 64, i & 127)); }
  return a;
}

/* ---------------- the twenty C-parity kernels (k01..k20) ---------------- */

static int32_t NOINL kv_fib(int32_t n) { return n < 2 ? n : (int32_t)((uint32_t)kv_fib(n - 1) + (uint32_t)kv_fib(n - 2)); }
static int32_t NOINL kv_add(int32_t a, int32_t b) { return (int32_t)((uint32_t)a + (uint32_t)b); }
static int32_t kv_gcd(int32_t a, int32_t b) { while (b != 0) { int32_t t = a % b; a = b; b = t; } return a; }
struct kv_node { int32_t val; struct kv_node *next; };
struct kv_rect { int32_t w, h; };
struct kv_point { double x, y; };

static double NOINL op_k01_sum_or(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)i); }
  return s;
}
static double NOINL op_k02_fib(long n) {
  int32_t N = (int32_t)n, s = 0, ten = 10; OPAQUE_I(ten);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)kv_fib(ten)); }
  return s;
}
/* Kernel 3's target is the call inlined away: the twin's add is inlined. */
static inline int32_t kv_add_inline(int32_t a, int32_t b) { return (int32_t)((uint32_t)a + (uint32_t)b); }
static double NOINL op_k03_call_add(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = kv_add_inline(s, i); }
  (void)kv_add;
  return s;
}
static double NOINL op_k04_dot(long n) {
  int32_t N = (int32_t)n; double s = 0;
  struct kv_point o = {1.5, 2.25}, *p = &o; OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s += p->x * p->x + p->y * p->y; }
  return s;
}
static double NOINL op_k05_field_add(long n) {
  int32_t N = (int32_t)n; double dx = 0.25; OPAQUE_D(dx);
  struct kv_point o = {0.5, 0}, *p = &o; OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); p->x += dx; }
  return p->x;
}
static double NOINL op_k06_list(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct kv_node *head = NULL;
  for (int32_t j = 15; j >= 0; j--) { struct kv_node *k = malloc(sizeof *k); k->val = j; k->next = head; head = k; }
  ESCAPE(head);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); for (struct kv_node *p = head; p; p = p->next) s = (int32_t)((uint32_t)s + (uint32_t)p->val); }
  while (head) { struct kv_node *k = head->next; free(head); head = k; }
  return s;
}
static double NOINL op_k07_area(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct kv_rect o = {3, 4}, *r = &o; OPAQUE_P(r);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)r->w * (uint32_t)r->h); }
  return s;
}
static double NOINL op_k08_escape(long n) {
  int32_t N = (int32_t)n; double s = 0;
  struct kv_point A = {1.5, 2}, B = {0.25, 3}, *a = &A, *b = &B; OPAQUE_P(a); OPAQUE_P(b);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); double vx = a->x + b->x, vy = a->y + b->y; s += vx * vy; }
  return s;
}
static double NOINL op_k09_reduce(long n) {
  int32_t N = (int32_t)n, s = 0; double arr[16];
  for (int32_t j = 0; j < 16; j++) arr[j] = j;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); double t = 0; for (int32_t j = 0; j < 16; j++) t += arr[j]; s = (int32_t)(uint32_t)(int64_t)((double)s + t); }
  return s;
}
static double NOINL op_k10_f64_sum(long n) {
  int32_t N = (int32_t)n; double s = 0, a[16];
  for (int32_t j = 0; j < 16; j++) a[j] = j + 0.5;
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); for (int32_t j = 0; j < 16; j++) s += a[j]; }
  return s;
}
static double NOINL op_k11_saxpy(long n) {
  int32_t N = (int32_t)n; double x[16], y[16], k = 0.5; OPAQUE_D(k);
  for (int32_t j = 0; j < 16; j++) { x[j] = j; y[j] = 0; }
  ESCAPE(x); ESCAPE(y);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); for (int32_t j = 0; j < 16; j++) y[j] += k * x[j]; }
  return y[15];
}
static double NOINL op_k12_copy(long n) {
  int32_t N = (int32_t)n, s = 0, src[16], dst[16];
  for (int32_t j = 0; j < 16; j++) src[j] = j;
  ESCAPE(src); ESCAPE(dst);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); src[i & 15] = i;
    memcpy(dst, src, sizeof dst); ESCAPE(dst);
    s = (int32_t)((uint32_t)s + (uint32_t)dst[i & 15]);
  }
  return s;
}
static double NOINL op_k13_matmul(long n) {
  int32_t N = (int32_t)n; double a[16], b[16], c[16];
  for (int32_t j = 0; j < 16; j++) { a[j] = j * 0.25; b[j] = 1 - j * 0.125; }
  ESCAPE(a); ESCAPE(b); ESCAPE(c);
  for (int32_t r = 0; r < N; r++) {
    BODY_BARRIER();
    for (int32_t i = 0; i < 4; i++) for (int32_t j = 0; j < 4; j++) { double t = 0; for (int32_t k = 0; k < 4; k++) t += a[i * 4 + k] * b[k * 4 + j]; c[i * 4 + j] = t; }
    a[r & 15] = (r & 7) * 0.125;
  }
  return c[5];
}
static double NOINL op_k14_bsearch(long n) {
  int32_t N = (int32_t)n, s = 0, a[64];
  for (int32_t j = 0; j < 64; j++) a[j] = j * 2;
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t key = (int32_t)((uint32_t)i * 7u) & 127, lo = 0, hi = 64;
    while (lo < hi) { int32_t mid = (int32_t)(((uint32_t)lo + (uint32_t)hi) >> 1); if (a[mid] < key) lo = mid + 1; else hi = mid; }
    s = (int32_t)((uint32_t)s + (uint32_t)lo);
  }
  return s;
}
static double NOINL op_k15_isort(long n) {
  int32_t N = (int32_t)n, s = 0, a[8];
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    for (int32_t j = 0; j < 8; j++) a[j] = ((j * 5 + i) & 7) * 3;
    for (int32_t j = 1; j < 8; j++) { int32_t v = a[j], k = j - 1; while (k >= 0 && a[k] > v) { a[k + 1] = a[k]; k--; } a[k + 1] = v; }
    s = (int32_t)((uint32_t)s + (uint32_t)a[i & 7]);
  }
  return s;
}
static double NOINL op_k16_sieve(long n) {
  int32_t N = (int32_t)n, s = 0; uint8_t f[64];
  ESCAPE(f);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    memset(f, 0, sizeof f); ESCAPE(f);
    for (int32_t p = 2; p * p < 64; p++) if (f[p] == 0) for (int32_t m = p * p; m < 64; m += p) f[m] = 1;
    s = (int32_t)((uint32_t)s + f[i & 63]);
  }
  return s;
}
static double NOINL op_k17_hash(long n) {
  int32_t N = (int32_t)n, h = 0;
  const char *str = "abcdefghijklmnop"; size_t len = 16; OPAQUE_P(str); OPAQUE_I(len);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); for (size_t j = 0; j < len; j++) h = (int32_t)((uint32_t)h * 31u + (uint8_t)str[j]); }
  return h;
}
static double NOINL op_k18_math(long n) {
  int32_t N = (int32_t)n; double s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    double x = i * 0.5 + 0.25;
    s += sqrt(x) + fabs(x - 100) + floor(x) + (double)(i == 0 ? 32 : __builtin_clz((uint32_t)i)) + (double)((uint32_t)i >> 3);
  }
  return s;
}
static double NOINL op_k19_gcd(long n) {
  int32_t N = (int32_t)n, s = 0, b = 360, d = 7; OPAQUE_I(b); OPAQUE_I(d);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); int32_t a = (i & 1023) + 1000; s = (int32_t)((uint32_t)s + (uint32_t)kv_gcd(a, b) + (uint32_t)(a / d)); }
  return s;
}
static double NOINL op_k20_switch(long n) {
  int32_t N = (int32_t)n, acc = 1; uint8_t code[16];
  for (int32_t j = 0; j < 16; j++) code[j] = (uint8_t)((j * 7 + 3) & 3);
  ESCAPE(code);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    for (int32_t pc = 0; pc < 16; pc++) {
      switch (code[pc]) {
        case 0: acc = (int32_t)((uint32_t)acc + 1u); break;
        case 1: acc ^= 0x5a; break;
        case 2: acc = (int32_t)((uint32_t)acc * 3u); break;
        case 3: acc >>= 1; break;
      }
    }
  }
  return acc;
}

/* Builtin twins (bi_*): what the builtin's body is in C, nothing more. */
static double NOINL op_bi_dv_get_f64(long n) {
  int32_t N = (int32_t)n; unsigned char buf[64]; double s = 0;
  for (int j = 0; j < 8; j++) { double v = j + 0.5; memcpy(buf + j * 8, &v, 8); }
  ESCAPE(buf);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); size_t at = (size_t)(i & 7) * 8; if (at + 8 > sizeof buf) abort(); double v; memcpy(&v, buf + at, 8); s += v; }
  return s;
}
static double NOINL op_bi_dv_set_f64(long n) {
  int32_t N = (int32_t)n; unsigned char buf[64] = {0};
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); size_t at = (size_t)(i & 7) * 8; if (at + 8 > sizeof buf) abort(); double v = i; memcpy(buf + at, &v, 8); ESCAPE(buf); }
  double v; memcpy(&v, buf, 8); return v;
}
static double NOINL op_bi_dv_get_u32(long n) {
  int32_t N = (int32_t)n; unsigned char buf[64]; int32_t a = 0;
  for (int j = 0; j < 16; j++) { uint32_t v = __builtin_bswap32((uint32_t)(j * 7)); memcpy(buf + j * 4, &v, 4); }
  ESCAPE(buf);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); size_t at = (size_t)(i & 15) * 4; if (at + 4 > sizeof buf) abort(); uint32_t v; memcpy(&v, buf + at, 4); a = (int32_t)((uint32_t)a + __builtin_bswap32(v)); }
  return a;
}
static double NOINL op_bi_arr_slice(long n) {
  int32_t N = (int32_t)n, a = 0; double arr[16];
  for (int j = 0; j < 16; j++) arr[j] = j;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); size_t len = 12 - 4; double *out = malloc(len * sizeof *out); memcpy(out, arr + 4, len * sizeof *out); ESCAPE(out); a += (int32_t)len; free(out); }
  return a;
}
static double NOINL op_bi_arr_concat(long n) {
  int32_t N = (int32_t)n, a = 0; double x[4] = {1, 2, 3, 4}, y[4] = {5, 6, 7, 8};
  ESCAPE(x); ESCAPE(y);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); double *out = malloc(8 * sizeof *out); memcpy(out, x, sizeof x); memcpy(out + 4, y, sizeof y); ESCAPE(out); a += 8; free(out); }
  return a;
}
static double NOINL op_bi_tmpl(long n) {
  int32_t N = (int32_t)n, a = 0; const char *p = "ab", *q = "xyz";
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    char *out = malloc(16), digits[12]; size_t len = 0; int d = 0; uint32_t v = (uint32_t)(i & 1023);
    do { digits[d++] = (char)('0' + v % 10); v /= 10; } while (v);
    memcpy(out, p, 2); len = 2; out[len++] = '-';
    while (d) out[len++] = digits[--d];
    out[len++] = '-'; memcpy(out + len, q, 3); len += 3;
    ESCAPE(out); a += (int32_t)len; free(out);
  }
  return a;
}
static int32_t bi_keys[128], bi_vals[128];
static int bi_probe(int32_t key) { uint32_t h = (uint32_t)key * 2654435769u >> 25; while (bi_keys[h] != key && bi_keys[h] != -1) h = (h + 1) & 127; return (int)h; }
static double NOINL op_bi_map_get(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int j = 0; j < 128; j++) bi_keys[j] = -1;
  for (int j = 0; j < 64; j++) { int h = bi_probe(j); bi_keys[h] = j; bi_vals[h] = j * 3; }
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); int h = bi_probe(i & 63); a = (int32_t)((uint32_t)a + (uint32_t)(bi_keys[h] == -1 ? 0 : bi_vals[h])); }
  return a;
}
static double NOINL op_bi_set_has(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int j = 0; j < 128; j++) bi_keys[j] = -1;
  for (int j = 0; j < 64; j += 2) bi_keys[bi_probe(j)] = j;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (bi_keys[bi_probe(i & 63)] != -1) a++; }
  return a;
}

/* bi_* twins, batch 2. Fragment: paste after op_bi_set_has in ops.c (uses its includes and macros:
 * NOINL, BODY_BARRIER, ESCAPE; headers stdint, stdio, stdlib, string). C99, no extra headers. */

static int bia_is_ws(char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f'; }
static int bia_cmp_int(const void *x, const void *y) { int32_t p = *(const int32_t *)x, q = *(const int32_t *)y; return (p > q) - (p < q); }
typedef struct { int32_t a, b, c, d; } bia_obj4;

static double NOINL op_bi_fill(long n) {
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    double *arr = malloc(16 * sizeof *arr);
    for (int j = 0; j < 16; j++) arr[j] = 0;
    ESCAPE(arr); a += 16; free(arr);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_join(long n) {
  static const char *parts[4] = {"a", "bb", "ccc", "dddd"};
  static const size_t plen[4] = {1, 2, 3, 4};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    char *out = malloc(16); size_t len = 0;
    for (int k = 0; k < 4; k++) {
      if (k) out[len++] = '-';
      memcpy(out + len, parts[k], plen[k]); len += plen[k];
    }
    out[len] = '\0'; ESCAPE(out); a += (uint32_t)len; free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_from(long n) {
  static const char *words[4] = {"a", "bb", "ccc", "dddd"};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const char **out = malloc(4 * sizeof *out);
    for (int k = 0; k < 4; k++) out[k] = words[k];
    ESCAPE(out); a += 4; free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_upper(long n) {
  static const char s[] = "hello";
  size_t len = sizeof s - 1;
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(s);
    char *out = malloc(len + 1);
    for (size_t k = 0; k < len; k++) { char c = s[k]; out[k] = (c >= 'a' && c <= 'z') ? (char)(c - 'a' + 'A') : c; }
    out[len] = '\0'; ESCAPE(out); a += (uint32_t)len; free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_trim(long n) {
  static const char s[] = "  abc  ";
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(s);
    size_t lo = 0, hi = sizeof s - 1;
    while (lo < hi && bia_is_ws(s[lo])) lo++;
    while (hi > lo && bia_is_ws(s[hi - 1])) hi--;
    char *out = malloc(hi - lo + 1);
    memcpy(out, s + lo, hi - lo); out[hi - lo] = '\0';
    ESCAPE(out); a += (uint32_t)(hi - lo); free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_pad(long n) {
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    char digs[12]; int nd = 0; unsigned v = (unsigned)(i & 7);
    do { digs[nd++] = (char)('0' + v % 10); v /= 10; } while (v);
    char *out = malloc(16); size_t len = 0;
    while (len + (size_t)nd < 3) out[len++] = '0';
    while (nd) out[len++] = digs[--nd];
    out[len] = '\0'; ESCAPE(out); a += (uint32_t)len; free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_sindex(long n) {
  /* Writable text and an escaped needle: strstr of two constants folds away at compile time. */
  static char s[] = "alpha,beta,gamma,delta";
  static char needle[] = "gam";
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(s); ESCAPE(needle);
    const char *hit = strstr(s, needle);
    a += (uint32_t)(hit ? (int32_t)(hit - s) : -1);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_sslice(long n) {
  static const char s[] = "alpha,beta,gamma,delta";
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(s);
    size_t len = 10 - 6;
    char *out = malloc(len + 1);
    memcpy(out, s + 6, len); out[len] = '\0';
    ESCAPE(out); a += (uint32_t)len; free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_split(long n) {
  static const char s[] = "alpha,beta,gamma,delta";
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(s);
    char *buf = malloc(sizeof s); memcpy(buf, s, sizeof s);
    char **out = malloc(4 * sizeof *out);
    int k = 0; out[k++] = buf;
    for (char *q = buf; *q; q++) if (*q == ',') { *q = '\0'; out[k++] = q + 1; }
    ESCAPE(buf); ESCAPE(out); a += (uint32_t)k; free(out); free(buf);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_replace(long n) {
  static const char s[] = "alpha,beta,gamma,delta";
  static const char rep[] = "B";
  const char *pat = "beta";
  size_t slen = sizeof s - 1, plen = 4, rlen = 1;
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(s);
    const char *hit = strstr(s, pat);
    size_t at = (size_t)(hit - s);
    size_t len = slen - plen + rlen;
    char *out = malloc(len + 1);
    memcpy(out, s, at); memcpy(out + at, rep, rlen);
    memcpy(out + at + rlen, s + at + plen, slen - at - plen); out[len] = '\0';
    ESCAPE(out); a += (uint32_t)len; free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_sort_int(long n) {
  static const int32_t src[16] = {11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t *arr = malloc(16 * sizeof *arr);
    memcpy(arr, src, sizeof src);
    ESCAPE(arr);
    char keys[16][12];
    for (int j = 0; j < 16; j++) snprintf(keys[j], sizeof keys[j], "%d", (int)arr[j]);
    /* insertion sort of (key, value) pairs by decimal text, i.e. JS default sort order */
    for (int j = 1; j < 16; j++) {
      char kbuf[12]; int32_t v = arr[j];
      memcpy(kbuf, keys[j], sizeof kbuf);
      int k = j - 1;
      while (k >= 0 && strcmp(keys[k], kbuf) > 0) {
        memcpy(keys[k + 1], keys[k], sizeof kbuf); arr[k + 1] = arr[k]; k--;
      }
      memcpy(keys[k + 1], kbuf, sizeof kbuf); arr[k + 1] = v;
    }
    ESCAPE(arr); a += (uint32_t)arr[0]; free(arr);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_sort_cmp(long n) {
  static const int32_t src[16] = {11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t *arr = malloc(16 * sizeof *arr);
    memcpy(arr, src, sizeof src);
    ESCAPE(arr);
    qsort(arr, 16, sizeof *arr, bia_cmp_int);
    ESCAPE(arr); a += (uint32_t)arr[0]; free(arr);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_map(long n) {
  static const double src[16] = {11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    double *out = malloc(16 * sizeof *out);
    for (int j = 0; j < 16; j++) out[j] = src[j] + 1;
    ESCAPE(out); a += (uint32_t)(int32_t)out[0]; free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_parse(long n) {
  static const char text[] = "{\"a\":1,\"b\":[1,2,3],\"c\":\"xyz\"}";
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(text);
    const char *p = text + 1; /* past '{' */
    int av = 0;
    double *arr = NULL; char *str = NULL;
    while (*p != '}') {
      char key = p[1];          /* p[0] is the key's opening quote */
      p += 4;                   /* skip  "k":  */
      if (*p == '[') {
        arr = malloc(3 * sizeof *arr);
        int k = 0; p++;
        while (*p != ']') { if (*p >= '0' && *p <= '9') arr[k++] = (double)(*p - '0'); p++; }
        p++;
      } else if (*p == '"') {
        const char *st = ++p;
        while (*p != '"') p++;
        size_t len = (size_t)(p - st);
        str = malloc(len + 1); memcpy(str, st, len); str[len] = '\0';
        p++;
      } else {                  /* single-digit number */
        if (key == 'a') av = *p - '0';
        p++;
      }
      if (*p == ',') p++;
    }
    a += (uint32_t)av;
    ESCAPE(arr); ESCAPE(str); free(arr); free(str);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_keys(long n) {
  static const char *names[4] = {"a", "b", "c", "d"};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const char **out = malloc(4 * sizeof *out);
    for (int k = 0; k < 4; k++) out[k] = names[k];
    ESCAPE(out); a += 4; free(out);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_assign(long n) {
  static const bia_obj4 o = {1, 2, 3, 4};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    bia_obj4 *out = malloc(sizeof *out);
    out->a = o.a; out->b = o.b; out->c = o.c; out->d = o.d;
    ESCAPE(out); a += (uint32_t)out->a; free(out);
  }
  return (double)(int32_t)a;
}


/* bi_* twins, batch 3: the eleven builtins added to ops.js. Writable statics, not static const. */
static int bi3_match_abc(const char *t, size_t n, size_t *start) {
  for (size_t s = 0; s < n; s++) {
    if (t[s] != 'a') continue;
    size_t p = s + 1;
    if (p >= n || t[p] != 'b') continue;
    while (p < n && t[p] == 'b') p++;
    if (p < n && t[p] == 'c') { *start = s; return 1; }
  }
  return 0;
}

static double NOINL op_bi_arr_at(long n) {
  static int32_t arr16[16] = {11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); ESCAPE(arr16); a = a + (uint32_t)arr16[i & 15]; }
  return (double)(int32_t)a;
}

static double NOINL op_bi_arr_lastindexof(long n) {
  static int32_t arr16[16] = {11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(arr16);
    int32_t idx = -1;
    for (int j = 15; j >= 0; j--) if (arr16[j] == 4) { idx = j; break; }
    a = a + (uint32_t)idx;
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_arr_reverse(long n) {
  static int32_t arr16[16] = {11, 48, 85, 21, 58, 95, 31, 68, 4, 42, 79, 15, 52, 89, 25, 62};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(arr16);
    for (int j = 0; j < 8; j++) { int32_t t = arr16[j]; arr16[j] = arr16[15 - j]; arr16[15 - j] = t; }
    a = a + (uint32_t)arr16[0];
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_num_tofixed(long n) {
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    char buf[32]; double x = (double)i * 0.5;
    int len = snprintf(buf, sizeof buf, "%.2f", x);
    ESCAPE(buf); a += (uint32_t)len;
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_num_radix(long n) {
  int32_t N = (int32_t)n; uint32_t a = 0;
  static char hex[] = "0123456789abcdef";
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(hex);
    char digs[8]; int nd = 0; uint32_t v = (uint32_t)(i & 1023);
    do { digs[nd++] = hex[v & 15]; v >>= 4; } while (v);
    ESCAPE(digs); a += (uint32_t)nd;
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_getproto(long n) {
  static struct { const void *proto; } object_proto, o;
  int32_t N = (int32_t)n; uint32_t a = 0;
  o.proto = &object_proto;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(&o);
    a = a + (uint32_t)(o.proto == &object_proto);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_u8_fill(long n) {
  static uint8_t buf[64];
  int32_t N = (int32_t)n; uint32_t a = 0;
  ESCAPE(buf);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    memset(buf, (int)(i & 255), sizeof buf); ESCAPE(buf);
    a += buf[0];
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_f64_indexof(long n) {
  static double f64[16];
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int j = 0; j < 16; j++) f64[j] = j * 0.5;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(f64);
    double needle = (double)(i & 15) * 0.5;
    int idx = -1;
    for (int j = 0; j < 16; j++) if (f64[j] == needle) { idx = j; break; }
    a += (uint32_t)idx;
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_regexp_test(long n) {
  static char text[] = "xxabbbc";
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(text);
    size_t start = 0;
    a += (uint32_t)bi3_match_abc(text, sizeof text - 1, &start);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_regexp_exec(long n) {
  static char text[] = "xxabbbc";
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(text);
    size_t *res = malloc(sizeof *res); /* the match array exec allocates */
    size_t start = 0;
    if (bi3_match_abc(text, sizeof text - 1, &start)) res[0] = start; else res[0] = 0;
    ESCAPE(res); a += (uint32_t)res[0]; free(res);
  }
  return (double)(int32_t)a;
}

static double NOINL op_bi_obj_keys2(long n) {
  static const char *names[2] = {"a", "b"};
  int32_t N = (int32_t)n; uint32_t a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const char **out = malloc(2 * sizeof *out);
    for (int k = 0; k < 2; k++) out[k] = names[k];
    ESCAPE(out); a += 2; free(out);
  }
  return (double)(int32_t)a;
}

/* ---------------- C-shaped constructs (inventory 2026-11-21) ---------------- */

static double NOINL op_c_while(long n) {
  int32_t N = (int32_t)n, s = 0, i = 0;
  while (i < N) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)i); i++; }
  return s;
}

static double NOINL op_c_do_while(long n) {
  int32_t N = (int32_t)n, s = 0, i = 0;
  if (N > 0) { do { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)i); i++; } while (i < N); }
  return s;
}

static double NOINL op_c_for_down(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = N - 1; i >= 0; i--) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)i); }
  return s;
}

static double NOINL op_c_loop_step2(long n) {
  int32_t N = (int32_t)n, s = 0;
  double j = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)(uint64_t)j); j = j + 2.0; }
  return s;
}

/* Outer runs n >> 4 times, inner runs 16 times: total inner work is ~n. */
static double NOINL op_c_nested_loop(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t M = N >> 4;
  for (int32_t i = 0; i < M; i++) {
    for (int32_t j = 0; j < 16; j++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)i * (uint32_t)j); }
  }
  return s;
}

static double NOINL op_c_loop_break(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t arr[16];
  for (int32_t k = 0; k < 16; k++) arr[k] = (k * 5 + 3) & 15;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t v = i & 15, k;
    for (k = 0; k < 16; k++) { if (arr[k] == v) break; }
    s = (int32_t)((uint32_t)s + (uint32_t)k);
  }
  return s;
}

static double NOINL op_c_loop_continue(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (i & 1) continue; s = (int32_t)((uint32_t)s + (uint32_t)i); }
  return s;
}

static double NOINL op_c_loop_if_else(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if ((i & 1) == 0) s = (int32_t)((uint32_t)s + (uint32_t)i);
    else s = (int32_t)((uint32_t)s - 3u);
  }
  return s;
}

static double NOINL op_c_loop_if_chain(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t k = i & 3;
    if (k == 0) s = (int32_t)((uint32_t)s + (uint32_t)i);
    else if (k == 1) s = s ^ i;
    else s = (int32_t)((uint32_t)s * 3u + 1u);
  }
  return s;
}

static double NOINL op_c_ternary(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)((i & 1) ? 7 : i)); }
  return s;
}

static double NOINL op_c_select_minmax(long n) {
  int32_t N = (int32_t)n, m = 0, m2 = 1023;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t v = (int32_t)(((uint32_t)i * 7919u) & 1023u);
    if (v > m) m = v;
    if (v < m2) m2 = v;
  }
  return (double)(m * 1024 + m2);
}

static double NOINL op_c_switch_dense(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    switch (i & 7) {
      case 0: s = (int32_t)((uint32_t)s + 1u); break;
      case 1: s = (int32_t)((uint32_t)s + 3u); break;
      case 2: s = s ^ i; break;
      case 3: s = (int32_t)((uint32_t)s * 3u); break;
      case 4: s = (int32_t)((uint32_t)s - (uint32_t)i); break;
      case 5: s = (int32_t)((uint32_t)s << 1); break;
      case 6: s = (s >> 1) ^ i; break;
      case 7: s = (int32_t)((uint32_t)s + 7u); break;
    }
  }
  return s;
}

static double NOINL op_c_switch_sparse(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    switch ((i & 7) * 100) {
      case 0: s = (int32_t)((uint32_t)s + 1u); break;
      case 100: s = (int32_t)((uint32_t)s + 5u); break;
      case 300: s = s ^ i; break;
      case 700: s = (int32_t)((uint32_t)s * 3u + (uint32_t)i); break;
      default: s = (int32_t)((uint32_t)s - 2u); break;
    }
  }
  return s;
}

static int32_t NOINL c_early_return_helper(const int32_t *arr, int32_t v) {
  for (int32_t k = 0; k < 16; k++) { if (arr[k] == v) return k; }
  return -1;
}

static double NOINL op_c_early_return(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t arr[16];
  for (int32_t k = 0; k < 16; k++) arr[k] = (k * 5 + 3) & 15;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)((uint32_t)s + (uint32_t)c_early_return_helper(arr, i & 15));
  }
  return s;
}

static double NOINL op_c_bool_flag(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t f = (i & 3) == 0;
    if (f && i > 10) c = c + 1.0;
  }
  return c;
}

static double NOINL op_c_logical_and_or(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (((i & 1) && (i & 2)) || i == 5) c = c + 1.0;
  }
  return c;
}

static double NOINL op_c_locals_rotate(long n) {
  int32_t N = (int32_t)n, a = 0, b = 1, c = 2;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t t = a;
    a = b;
    b = c;
    c = (int32_t)((uint32_t)t + (uint32_t)i);
  }
  return (double)(int32_t)((uint32_t)a + (uint32_t)b + (uint32_t)c);
}

static double NOINL op_c_int_expr(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)(((uint32_t)s ^ ((uint32_t)i * 3u)) + ((uint32_t)i >> 2) - ((uint32_t)i & 5u));
  }
  return s;
}

/* ---------------- C-shaped constructs (group b) ---------------- */

static double NOINL op_c_arr_sum_int(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t a[256];
  for (int32_t k = 0; k < 256; k++) a[k] = k * 7 - 300;
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)a[i & 255]); }
  return s;
}

static double NOINL op_c_arr_sum_dbl(long n) {
  int32_t N = (int32_t)n;
  double s = 0, a[256];
  for (int32_t k = 0; k < 256; k++) a[k] = k + 0.5;
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s += a[i & 255]; }
  return s;
}

static double NOINL op_c_arr_store_seq(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t a[256];
  for (int32_t k = 0; k < 256; k++) a[k] = 0;
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a[i & 255] = i; }
  for (int32_t k = 0; k < 256; k++) s = (int32_t)((uint32_t)s + (uint32_t)a[k]);
  return s;
}

static double NOINL op_c_arr_rmw(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t a[256];
  for (int32_t k = 0; k < 256; k++) a[k] = k;
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a[i & 255] = (int32_t)((uint32_t)a[i & 255] + 1u); }
  for (int32_t k = 0; k < 256; k++) s = (int32_t)((uint32_t)s + (uint32_t)a[k]);
  return s;
}

static double NOINL op_c_arr_stencil(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t a[256];
  for (int32_t k = 0; k < 256; k++) a[k] = k * 3 + 1;
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t j = 1 + (i & 253);
    s = (int32_t)((uint32_t)s + (uint32_t)a[j - 1] + (uint32_t)a[j] + (uint32_t)a[j + 1]);
  }
  return s;
}

static double NOINL op_c_arr_swap(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t a[256];
  for (int32_t k = 0; k < 256; k++) a[k] = k;
  ESCAPE(a);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t p = i & 255, q = (i * 7) & 255;
    int32_t t = a[p];
    a[p] = a[q];
    a[q] = t;
  }
  for (int32_t k = 0; k < 256; k++) s = (int32_t)((uint32_t)s + (uint32_t)a[k] * (uint32_t)(k + 1));
  return s;
}

static double NOINL op_c_arr_indirect(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t a[256], b[256];
  for (int32_t k = 0; k < 256; k++) { a[k] = k * 3 + 1; b[k] = (k * 37 + 11) & 255; }
  ESCAPE(a); ESCAPE(b);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)a[b[i & 255]]); }
  return s;
}

static double NOINL op_c_arr_histogram(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t a[256], h[16];
  for (int32_t k = 0; k < 256; k++) a[k] = k * 5 + 3;
  for (int32_t k = 0; k < 16; k++) h[k] = 0;
  ESCAPE(a); ESCAPE(h);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); h[a[i & 255] & 15]++; }
  for (int32_t k = 0; k < 16; k++) s = (int32_t)((uint32_t)s + (uint32_t)h[k] * (uint32_t)(k + 1));
  return s;
}

static double NOINL op_c_arr_2d(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t m[16][16];
  for (int32_t r = 0; r < 16; r++)
    for (int32_t c = 0; c < 16; c++) m[r][c] = r * 16 + c;
  ESCAPE(m);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)m[(i >> 4) & 15][i & 15]); }
  return s;
}

static double NOINL op_c_arr_len_loop(long n) {
  /* outer n>>8, inner 256: total inner work (n>>8)*256 ~ n */
  int32_t N = (int32_t)n, s = 0, len = 256;
  int32_t a[256];
  for (int32_t k = 0; k < 256; k++) a[k] = k * 9 - 50;
  ESCAPE(a); OPAQUE_I(len);
  for (int32_t o = 0; o < (N >> 8); o++)
    for (int32_t j = 0; j < len; j++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)a[j]); }
  return s;
}

static double NOINL op_c_i32_sum(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t t[256];
  for (int32_t k = 0; k < 256; k++) t[k] = k * 3 - 100;
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)t[i & 255]); }
  return s;
}

static double NOINL op_c_i32_store(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t t[256];
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); t[i & 255] = i; }
  for (int32_t k = 0; k < 256; k++) s = (int32_t)((uint32_t)s + (uint32_t)t[k]);
  return s;
}

static double NOINL op_c_u8_rmw(long n) {
  int32_t N = (int32_t)n, s = 0;
  uint8_t u[256];
  for (int32_t k = 0; k < 256; k++) u[k] = (uint8_t)k;
  ESCAPE(u);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); u[i & 255] = (uint8_t)((u[i & 255] + 3) & 255); }
  for (int32_t k = 0; k < 256; k++) s = (int32_t)((uint32_t)s + (uint32_t)u[k]);
  return s;
}

static double NOINL op_c_u16_load(long n) {
  int32_t N = (int32_t)n, s = 0;
  uint16_t u[256];
  for (int32_t k = 0; k < 256; k++) u[k] = (uint16_t)(k * 257);
  ESCAPE(u);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)u[i & 255]); }
  return s;
}

static double NOINL op_c_f32_mad(long n) {
  int32_t N = (int32_t)n;
  float f[256];
  double s = 0;
  for (int32_t k = 0; k < 256; k++) f[k] = (float)(k * 0.25);
  ESCAPE(f);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t k = i & 255;
    f[k] = f[k] * 0.5f + 1.0f;
  }
  for (int32_t k = 0; k < 256; k++) s += f[k];
  return s;
}

static double NOINL op_c_f64_dot(long n) {
  int32_t N = (int32_t)n;
  double x[256], y[256], s = 0;
  for (int32_t k = 0; k < 256; k++) { x[k] = k * 0.5; y[k] = k * 0.25 + 1; }
  ESCAPE(x); ESCAPE(y);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); int32_t k = i & 255; s += x[k] * y[k]; }
  return s;
}

static double NOINL op_c_i32_indirect(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t idx[256], d[256];
  for (int32_t k = 0; k < 256; k++) { idx[k] = (k * 29 + 5) & 255; d[k] = k * k - 1000; }
  ESCAPE(idx); ESCAPE(d);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)d[idx[i & 255]]); }
  return s;
}

/* ---------------- c_ group: struct, call, closure and method shapes (twins of ops.js ops.c_*) ---------------- */
struct cs_rmw { int32_t count, sum; };
struct cs_nest_c { int32_t v; };
struct cs_nest_b { struct cs_nest_c c; };
struct cs_nest_a { struct cs_nest_b b; };
struct cs_dpt { double x, y; };
struct cs_p { int32_t x; };
struct cs_q { int32_t x, y; };
struct cs_cv { int32_t v; };
struct cs_poly { int32_t (*v)(const struct cs_poly *self); };

static int32_t NOINL cs_p_get(const struct cs_p *self) { return self->x; }
static void NOINL cs_q_add(struct cs_q *self, int32_t v) { self->x = (int32_t)((uint32_t)self->x + (uint32_t)v); }
static int32_t NOINL c_call_leaf_f(int32_t a, int32_t b) { return (int32_t)((uint32_t)a + (uint32_t)b); }
static int32_t NOINL c_call_4args_f(int32_t a, int32_t b, int32_t c, int32_t d) { return (int32_t)((uint32_t)a + (uint32_t)b + (uint32_t)c + (uint32_t)d); }
static int32_t NOINL c_call_2deep_g(int32_t x) { return (int32_t)((uint32_t)x + 1u); }
static int32_t NOINL c_call_2deep_f(int32_t x) { return c_call_2deep_g(x); }
static void NOINL c_call_void_f(struct cs_cv *o) { o->v = (int32_t)((uint32_t)o->v + 1u); }
static int32_t NOINL c_call_recursive_depth4_r(int32_t k, int32_t x) {
  if (k == 0) return (int32_t)((uint32_t)x + (uint32_t)k);
  int32_t v = c_call_recursive_depth4_r(k - 1, (int32_t)((uint32_t)x + (uint32_t)k));
  OPAQUE_I(v); /* keeps the recursive call a real call (no tail-call to loop rewrite) */
  return v;
}
static double NOINL c_call_dbl_f(double x) { return x * x; }
static int32_t NOINL c_closure_read_f(const struct env *e, int32_t x) { return (int32_t)((uint32_t)x + (uint32_t)e->k); }
static int32_t NOINL c_return_early_f(int32_t x) { if (x < 0) return -1; if (x > 0) return 1; return 0; }
static int32_t NOINL c_method_poly2_a_v(const struct cs_poly *self) { (void)self; return 1; }
static int32_t NOINL c_method_poly2_b_v(const struct cs_poly *self) { (void)self; return 2; }

static double NOINL op_c_struct_get2(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct pair p = {3, 4}, *q = &p;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); ESCAPE(q); s = (int32_t)((uint32_t)s + (uint32_t)q->x + (uint32_t)q->y); }
  return s;
}

static double NOINL op_c_struct_set(long n) {
  int32_t N = (int32_t)n;
  struct pair p = {0, 0}, *q = &p;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); ESCAPE(q); q->x = i; q->y = (int32_t)((uint32_t)q->y + (uint32_t)q->x); }
  return q->y;
}

static double NOINL op_c_struct_rmw(long n) {
  int32_t N = (int32_t)n;
  struct cs_rmw o = {0, 0}, *q = &o;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER(); ESCAPE(q);
    q->count = (int32_t)((uint32_t)q->count + 1u);
    q->sum = (int32_t)((uint32_t)q->sum + (uint32_t)i);
  }
  return (int32_t)((uint32_t)q->sum + (uint32_t)q->count);
}

static double NOINL op_c_struct_array(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct pair *pts[64];
  for (int32_t j = 0; j < 64; j++) { pts[j] = malloc(sizeof *pts[j]); pts[j]->x = j; pts[j]->y = -j; }
  ESCAPE(pts);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); ESCAPE(pts); s = (int32_t)((uint32_t)s + (uint32_t)pts[i & 63]->x); }
  for (int32_t j = 0; j < 64; j++) free(pts[j]);
  return s;
}

static double NOINL op_c_struct_nested(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct cs_nest_a a = {{{7}}}, *q = &a;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); ESCAPE(q); s = (int32_t)((uint32_t)s + (uint32_t)q->b.c.v); }
  return s;
}

static double NOINL op_c_struct_dbl(long n) {
  int32_t N = (int32_t)n;
  double s = 0;
  struct cs_dpt v = {0.5, 1.5}, *q = &v;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); ESCAPE(q); s += q->x * q->y; }
  return s;
}

static double NOINL op_c_class_method(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct cs_p p = {3}, *q = &p;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)cs_p_get(q)); }
  return s;
}

static double NOINL op_c_class_ctor_fields(long n) {
  int32_t N = (int32_t)n;
  struct cs_q p = {0, 1}, *q = &p;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); cs_q_add(q, i); }
  return (int32_t)((uint32_t)q->x + (uint32_t)q->y);
}

static double NOINL op_c_call_leaf(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = c_call_leaf_f(s, i); }
  return s;
}

static double NOINL op_c_call_4args(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = c_call_4args_f(s, i, i & 7, i >> 3); }
  return s;
}

static double NOINL op_c_call_2deep(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)c_call_2deep_f(i)); }
  return s;
}

static double NOINL op_c_call_void(long n) {
  int32_t N = (int32_t)n;
  struct cs_cv o = {0}, *q = &o;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); c_call_void_f(q); }
  return q->v;
}

static double NOINL op_c_call_recursive_depth4(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)c_call_recursive_depth4_r(4, i)); }
  return s;
}

static double NOINL op_c_call_dbl(long n) {
  int32_t N = (int32_t)n;
  double s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s += c_call_dbl_f((double)i * 0.5); }
  return s;
}

static double NOINL op_c_closure_read(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct env e = {7}, *p = &e;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)c_closure_read_f(p, i)); }
  return s;
}

static double NOINL op_c_method_poly2(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct cs_poly objs[2] = { { c_method_poly2_a_v }, { c_method_poly2_b_v } }, *q = objs;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const struct cs_poly *o = &q[i & 1];
    s = (int32_t)((uint32_t)s + (uint32_t)o->v(o));
  }
  return s;
}

static double NOINL op_c_return_early(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)c_return_early_f((i & 7) - 3)); }
  return s;
}

/* ---------------- C-shaped constructs (group d) ---------------- */

static double NOINL op_c_dbl_expr(long n) {
  int32_t N = (int32_t)n;
  double s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = s * 0.999 + (double)i * 0.5 - 1.25; }
  return s;
}

static double NOINL op_c_dbl_cmp_branch(long n) {
  int32_t N = (int32_t)n;
  double x = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    x += 0.25;
    if (x > 100.0) x -= 100.0;
  }
  return x;
}

static double NOINL op_c_dbl_to_int(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t t = (int32_t)((double)i * 0.37);
    s = (int32_t)((uint32_t)s + (uint32_t)t);
  }
  return s;
}

static double NOINL op_c_int_to_dbl(long n) {
  int32_t N = (int32_t)n;
  double d = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); d = d + (double)i; }
  return d;
}

static double NOINL op_c_int_div_const(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)(i / 4)); }
  return s;
}

static double NOINL op_c_int_mod_pow2(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); s = (int32_t)((uint32_t)s + (uint32_t)(i % 16)); }
  return s;
}

static double NOINL op_c_popcount(long n) {
  int32_t N = (int32_t)n, c = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t x = i & 1023;
    while (x != 0) { BODY_BARRIER(); x = x & (x - 1); c = (int32_t)((uint32_t)c + 1u); }
  }
  return c;
}

static double NOINL op_c_bit_test(long n) {
  int32_t N = (int32_t)n, mask = 0x55aa55aa;
  double c = 0;
  OPAQUE_I(mask);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if ((uint32_t)mask & (1u << (i & 31))) c = c + 1.0;
  }
  return c;
}

static double NOINL op_c_rotate(long n) {
  int32_t N = (int32_t)n;
  uint32_t x = 1;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); x = ((x << 5) | (x >> 27)) ^ (uint32_t)i; }
  return (double)x;
}

static double NOINL op_c_hash_mix(long n) {
  int32_t N = (int32_t)n, h = 1;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    h = (int32_t)((uint32_t)(h ^ i) * 0x9e3779b1u);
    h = (int32_t)((uint32_t)h ^ ((uint32_t)h >> 15));
  }
  return h;
}

static double NOINL op_c_abs_int(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t x = (i & 255) - 128;
    s = (int32_t)((uint32_t)s + (uint32_t)(x < 0 ? -x : x));
  }
  return s;
}

static double NOINL op_c_clamp(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t v = (int32_t)(((uint32_t)i * 7u) & 1023u);
    if (v < 100) v = 100;
    else if (v > 900) v = 900;
    s = (int32_t)((uint32_t)s + (uint32_t)v);
  }
  return s;
}

static double NOINL op_c_str_char_scan(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *s = S;
  OPAQUE_P(s);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    c = c + ((uint8_t)s[i % 54] == 32 ? 1.0 : 0.0);
  }
  return c;
}

static double NOINL op_c_str_hash(long n) {
  int32_t N = (int32_t)n, h = 0;
  const char *s = S;
  OPAQUE_P(s);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    h = (int32_t)((uint32_t)h * 31u + (uint32_t)(uint8_t)s[i % 54]);
  }
  return h;
}

static double NOINL op_c_str_index_eq(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *s = S;
  OPAQUE_P(s);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (s[i % 54] == 'o') c = c + 1.0;
  }
  return c;
}

static double NOINL op_c_str_len_loop(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *s = S;
  int32_t l = (int32_t)strlen(s);
  OPAQUE_P(s); OPAQUE_I(l);
  int32_t outer = N / 54; /* outer ~n/54 iterations, inner 54 chars: total inner work ~n */
  for (int32_t i = 0; i < outer; i++) {
    BODY_BARRIER();
    for (int32_t j = 0; j < l; j++) {
      BODY_BARRIER();
      if ((uint8_t)s[j] < 100) c = c + 1.0;
    }
  }
  return c;
}

static double NOINL op_c_counter_dec(long n) {
  int32_t k = (int32_t)n, s = 0;
  while (k > 0) { BODY_BARRIER(); k--; s = (int32_t)((uint32_t)s + (uint32_t)k); }
  return s;
}

typedef double (*opfn)(long);
/* ---------------- VM foundations: C equivalents (v_*) ---------------- */

/* ---- v_* group oa ---- */
/* ---------------- v_ objects / dispatch (V8 v7 + JetStream 3.0 foundational constructs) ---------------- */

struct v_pm { const struct v_pm_vt *vt; int32_t k; };
struct v_pm_vt { int32_t (*m)(const struct v_pm *self, int32_t x); };

/* equiv: base-class method body, reached through the object's vtable slot (JS: inherited one prototype level) */
static int32_t NOINL v_proto_method_A_m(const struct v_pm *self, int32_t x) {
  return (int32_t)((uint32_t)self->k + (uint32_t)x);
}
static const struct v_pm_vt v_pm_vt_A = { v_proto_method_A_m };

/* equiv: vtable call on a subclass instance whose vtable slot is inherited from the base (no own m) */
static double NOINL op_v_proto_method(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_pm b = { &v_pm_vt_A, 3 }, *q = &b;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(q);
    s = (int32_t)((uint32_t)s + (uint32_t)q->vt->m(q, i));
  }
  return (double)s;
}

struct v_pc3 { const struct v_pc3_vt *vt; int32_t g; int32_t own; };
struct v_pc3_vt { int32_t (*m)(const struct v_pc3 *self, int32_t x); };

/* equiv: method slot found in the ancestor vtable three levels up (JS: 3 prototype hops); reads the base-set field g */
static int32_t NOINL v_pc3_P_m(const struct v_pc3 *self, int32_t x) {
  return (int32_t)((uint32_t)x ^ (uint32_t)self->g);
}
static const struct v_pc3_vt v_pc3_vt_P = { v_pc3_P_m };

/* equiv: struct with a grandparent-ctor field g, an own field, and a vtable pointer; reads both fields and calls the slot */
static double NOINL op_v_proto_chain3_field(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_pc3 r = { &v_pc3_vt_P, 5, 2 }, *q = &r;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(q);
    s = (int32_t)((uint32_t)s + (uint32_t)q->own + (uint32_t)q->g + (uint32_t)q->vt->m(q, i));
  }
  return (double)s;
}

struct v_pm3_hdr { const struct v_pm3_vt *vt; };
struct v_pm3_vt { int32_t (*v)(const struct v_pm3_hdr *self, int32_t x); };
struct v_pm3_A { const struct v_pm3_vt *vt; int32_t k; };
struct v_pm3_B { const struct v_pm3_vt *vt; int32_t k; };
struct v_pm3_C { const struct v_pm3_vt *vt; int32_t k; };

/* equiv: class A's method; self is the shared header, cast to A's struct */
static int32_t NOINL v_pm3_A_v(const struct v_pm3_hdr *self, int32_t x) {
  const struct v_pm3_A *o = (const struct v_pm3_A *)self;
  return (int32_t)((uint32_t)x + (uint32_t)o->k);
}
/* equiv: class B's method */
static int32_t NOINL v_pm3_B_v(const struct v_pm3_hdr *self, int32_t x) {
  const struct v_pm3_B *o = (const struct v_pm3_B *)self;
  return (int32_t)((uint32_t)x ^ (uint32_t)o->k);
}
/* equiv: class C's method */
static int32_t NOINL v_pm3_C_v(const struct v_pm3_hdr *self, int32_t x) {
  const struct v_pm3_C *o = (const struct v_pm3_C *)self;
  return (int32_t)((uint32_t)x - (uint32_t)o->k);
}
static const struct v_pm3_vt v_pm3_vt_A = { v_pm3_A_v };
static const struct v_pm3_vt v_pm3_vt_B = { v_pm3_B_v };
static const struct v_pm3_vt v_pm3_vt_C = { v_pm3_C_v };

/* equiv: array of three struct pointers sharing a header; each call is an indirect call through the object's vtable */
static double NOINL op_v_poly_method3(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_pm3_A oa = { &v_pm3_vt_A, 1 };
  struct v_pm3_B ob = { &v_pm3_vt_B, 5 };
  struct v_pm3_C oc = { &v_pm3_vt_C, 2 };
  struct v_pm3_hdr *arr[3] = { (struct v_pm3_hdr *)&oa, (struct v_pm3_hdr *)&ob, (struct v_pm3_hdr *)&oc };
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(arr);
    const struct v_pm3_hdr *o = arr[i % 3];
    s = (int32_t)((uint32_t)s + (uint32_t)o->vt->v(o, i));
  }
  return (double)s;
}

struct v_pm8_hdr { const struct v_pm8_vt *vt; };
struct v_pm8_vt { int32_t (*v)(const struct v_pm8_hdr *self, int32_t x); };
struct v_pm8_s0 { const struct v_pm8_vt *vt; int32_t k; };
struct v_pm8_s1 { const struct v_pm8_vt *vt; int32_t k; };
struct v_pm8_s2 { const struct v_pm8_vt *vt; int32_t k; };
struct v_pm8_s3 { const struct v_pm8_vt *vt; int32_t k; };
struct v_pm8_s4 { const struct v_pm8_vt *vt; int32_t k; };
struct v_pm8_s5 { const struct v_pm8_vt *vt; int32_t k; };
struct v_pm8_s6 { const struct v_pm8_vt *vt; int32_t k; };
struct v_pm8_s7 { const struct v_pm8_vt *vt; int32_t k; };

/* equiv: shape 0 method (x + k) */
static int32_t NOINL v_pm8_S0_v(const struct v_pm8_hdr *self, int32_t x) { const struct v_pm8_s0 *o = (const struct v_pm8_s0 *)self; return (int32_t)((uint32_t)x + (uint32_t)o->k); }
/* equiv: shape 1 method (x ^ k) */
static int32_t NOINL v_pm8_S1_v(const struct v_pm8_hdr *self, int32_t x) { const struct v_pm8_s1 *o = (const struct v_pm8_s1 *)self; return (int32_t)((uint32_t)x ^ (uint32_t)o->k); }
/* equiv: shape 2 method (x - k) */
static int32_t NOINL v_pm8_S2_v(const struct v_pm8_hdr *self, int32_t x) { const struct v_pm8_s2 *o = (const struct v_pm8_s2 *)self; return (int32_t)((uint32_t)x - (uint32_t)o->k); }
/* equiv: shape 3 method (x + x + k) */
static int32_t NOINL v_pm8_S3_v(const struct v_pm8_hdr *self, int32_t x) { const struct v_pm8_s3 *o = (const struct v_pm8_s3 *)self; return (int32_t)((uint32_t)x + (uint32_t)x + (uint32_t)o->k); }
/* equiv: shape 4 method (x | k) */
static int32_t NOINL v_pm8_S4_v(const struct v_pm8_hdr *self, int32_t x) { const struct v_pm8_s4 *o = (const struct v_pm8_s4 *)self; return (int32_t)((uint32_t)x | (uint32_t)o->k); }
/* equiv: shape 5 method (k - x) */
static int32_t NOINL v_pm8_S5_v(const struct v_pm8_hdr *self, int32_t x) { const struct v_pm8_s5 *o = (const struct v_pm8_s5 *)self; return (int32_t)((uint32_t)o->k - (uint32_t)x); }
/* equiv: shape 6 method (x + k + k) */
static int32_t NOINL v_pm8_S6_v(const struct v_pm8_hdr *self, int32_t x) { const struct v_pm8_s6 *o = (const struct v_pm8_s6 *)self; return (int32_t)((uint32_t)x + (uint32_t)o->k + (uint32_t)o->k); }
/* equiv: shape 7 method (x ^ (k * 3)) */
static int32_t NOINL v_pm8_S7_v(const struct v_pm8_hdr *self, int32_t x) { const struct v_pm8_s7 *o = (const struct v_pm8_s7 *)self; return (int32_t)((uint32_t)x ^ ((uint32_t)o->k * 3u)); }

static const struct v_pm8_vt v_pm8_vt_0 = { v_pm8_S0_v };
static const struct v_pm8_vt v_pm8_vt_1 = { v_pm8_S1_v };
static const struct v_pm8_vt v_pm8_vt_2 = { v_pm8_S2_v };
static const struct v_pm8_vt v_pm8_vt_3 = { v_pm8_S3_v };
static const struct v_pm8_vt v_pm8_vt_4 = { v_pm8_S4_v };
static const struct v_pm8_vt v_pm8_vt_5 = { v_pm8_S5_v };
static const struct v_pm8_vt v_pm8_vt_6 = { v_pm8_S6_v };
static const struct v_pm8_vt v_pm8_vt_7 = { v_pm8_S7_v };

/* equiv: eight struct types sharing a vtable header; the site is megamorphic, so every call is an indirect call */
static double NOINL op_v_poly_method8(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_pm8_s0 o0 = { &v_pm8_vt_0, 1 };
  struct v_pm8_s1 o1 = { &v_pm8_vt_1, 3 };
  struct v_pm8_s2 o2 = { &v_pm8_vt_2, 2 };
  struct v_pm8_s3 o3 = { &v_pm8_vt_3, 7 };
  struct v_pm8_s4 o4 = { &v_pm8_vt_4, 4 };
  struct v_pm8_s5 o5 = { &v_pm8_vt_5, 9 };
  struct v_pm8_s6 o6 = { &v_pm8_vt_6, 5 };
  struct v_pm8_s7 o7 = { &v_pm8_vt_7, 2 };
  struct v_pm8_hdr *objs[8] = {
    (struct v_pm8_hdr *)&o0, (struct v_pm8_hdr *)&o1, (struct v_pm8_hdr *)&o2, (struct v_pm8_hdr *)&o3,
    (struct v_pm8_hdr *)&o4, (struct v_pm8_hdr *)&o5, (struct v_pm8_hdr *)&o6, (struct v_pm8_hdr *)&o7
  };
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(objs);
    const struct v_pm8_hdr *o = objs[i & 7];
    s = (int32_t)((uint32_t)s + (uint32_t)o->vt->v(o, i));
  }
  return (double)s;
}

struct v_sc { const struct v_sc_vt *vt; int32_t k; };
struct v_sc_vt { int32_t (*m)(const struct v_sc *self, int32_t x); };

/* equiv: base method A.m body */
static int32_t NOINL v_super_call_A_m(const struct v_sc *self, int32_t x) {
  return (int32_t)((uint32_t)x + (uint32_t)self->k);
}
/* equiv: subclass method; super.m(x) is a direct (non-virtual) call to the base body, then + 3 */
static int32_t NOINL v_super_call_B_m(const struct v_sc *self, int32_t x) {
  return (int32_t)((uint32_t)v_super_call_A_m(self, x) + 3u);
}
static const struct v_sc_vt v_sc_vt_B = { v_super_call_B_m };

/* equiv: vtable call to the overriding method, which calls the base body directly */
static double NOINL op_v_super_call(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_sc b = { &v_sc_vt_B, 4 }, *q = &b;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(q);
    s = (int32_t)((uint32_t)s + (uint32_t)q->vt->m(q, i));
  }
  return (double)s;
}

struct v_getter { int32_t _v; };

/* equiv: NOINL accessor function for `get v()`: reads the backing field and adds 1 */
static int32_t NOINL v_getter_v_get(const struct v_getter *self) {
  return (int32_t)((uint32_t)self->_v + 1u);
}

/* equiv: struct with a backing field, read through an accessor call */
static double NOINL op_v_getter(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_getter g = { 3 }, *q = &g;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(q);
    s = (int32_t)((uint32_t)s + (uint32_t)v_getter_v_get(q));
  }
  return (double)s;
}

struct v_setter { int32_t _v; };

/* equiv: NOINL mutator for `set v(x)`: backing field += x */
static void NOINL v_setter_v_set(struct v_setter *self, int32_t x) {
  self->_v = (int32_t)((uint32_t)self->_v + (uint32_t)x);
}

/* equiv: struct with a backing field, written through a mutator call */
static double NOINL op_v_setter(long n) {
  int32_t N = (int32_t)n;
  struct v_setter o = { 0 }, *q = &o;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    v_setter_v_set(q, i);
  }
  return (double)q->_v;
}

struct v_fp4_p0 { int32_t x, a; };
struct v_fp4_p1 { int32_t b, x; };
struct v_fp4_p2 { int32_t x, c, d; };
struct v_fp4_p3 { int32_t e, f, x; };

/* equiv: four struct layouts with x at different offsets; the site loads x through the per-shape address table */
static double NOINL op_v_field_poly4(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_fp4_p0 p0 = { 1, 0 };
  struct v_fp4_p1 p1 = { 0, 2 };
  struct v_fp4_p2 p2 = { 3, 0, 0 };
  struct v_fp4_p3 p3 = { 0, 0, 4 };
  int32_t *xs[4] = { &p0.x, &p1.x, &p2.x, &p3.x };
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(xs);
    s = (int32_t)((uint32_t)s + (uint32_t)*xs[i & 3]);
  }
  return (double)s;
}

struct v_io_cls { const struct v_io_cls *parent; };
struct v_io_obj { const struct v_io_cls *cls; };
static const struct v_io_cls v_io_cls_Object = { NULL };
static const struct v_io_cls v_io_cls_A = { &v_io_cls_Object };
static const struct v_io_cls v_io_cls_B = { &v_io_cls_A };
static const struct v_io_cls v_io_cls_C = { &v_io_cls_Object };

/* equiv: walk the object's class parent chain looking for k (JS instanceof semantics) */
static int32_t NOINL v_instanceof_check(const struct v_io_obj *o, const struct v_io_cls *k) {
  for (const struct v_io_cls *p = o->cls; p != NULL; p = p->parent) {
    if (p == k) return 1;
  }
  return 0;
}

/* equiv: four objects of classes A, B (extends A), C and Object; instanceof A is a parent-chain walk per iteration */
static double NOINL op_v_instanceof(long n) {
  int32_t N = (int32_t)n, c = 0;
  struct v_io_obj objs[4] = { { &v_io_cls_A }, { &v_io_cls_B }, { &v_io_cls_C }, { &v_io_cls_Object } };
  const struct v_io_cls *target = &v_io_cls_A;
  OPAQUE_P(target);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(objs);
    c = (int32_t)((uint32_t)c + (uint32_t)v_instanceof_check(&objs[i & 3], target));
  }
  return (double)c;
}

struct v_pt { int32_t x, y; };

/* equiv: JS constructor Point(x, y): initialise one arena slot */
static struct v_pt * NOINL v_new_point_make(struct v_pt *slot, int32_t x, int32_t y) {
  slot->x = x;
  slot->y = y;
  return slot;
}

/* equiv: arena of N Points allocated once per op call; iteration i bump-allocates slot i, then reads both fields */
static double NOINL op_v_new_point(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_pt *arena = (struct v_pt *)malloc(sizeof(struct v_pt) * (size_t)(N > 0 ? N : 1));
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct v_pt *p = v_new_point_make(&arena[i], i, (int32_t)((uint32_t)i + 1u));
    s = (int32_t)((uint32_t)s + (uint32_t)p->x + (uint32_t)p->y);
  }
  free(arena);
  return (double)s;
}

struct v_ol { int32_t a, b, c; };

/* equiv: object-literal initialiser {a: i, b: i + 1, c: i & 7} written into an arena slot */
static struct v_ol * NOINL v_obj_literal_make(struct v_ol *slot, int32_t i) {
  slot->a = i;
  slot->b = (int32_t)((uint32_t)i + 1u);
  slot->c = i & 7;
  return slot;
}

/* equiv: arena of N three-field records allocated once per op call; iteration i fills slot i and reads a, b, c */
static double NOINL op_v_obj_literal(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_ol *arena = (struct v_ol *)malloc(sizeof(struct v_ol) * (size_t)(N > 0 ? N : 1));
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const struct v_ol *o = v_obj_literal_make(&arena[i], i);
    s = (int32_t)((uint32_t)s + (uint32_t)o->a + (uint32_t)o->b - (uint32_t)o->c);
  }
  free(arena);
  return (double)s;
}

struct v_ll_node { int32_t v; struct v_ll_node *next; };

/* equiv: 256 nodes in a ring; each iteration is one dependent pointer load plus one field read */
static double NOINL op_v_linked_list_walk(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_ll_node nodes[256];
  for (int32_t k = 0; k < 256; k++) nodes[k].v = (k * 7 + 3) & 255;
  for (int32_t k = 0; k < 256; k++) nodes[k].next = &nodes[(k + 1) & 255];
  struct v_ll_node *node = &nodes[0];
  ESCAPE(nodes);
  OPAQUE_P(node);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)((uint32_t)s + (uint32_t)node->v);
    node = node->next;
  }
  return (double)s;
}

struct v_cc_env { int32_t a, b; };

/* equiv: closure body; the captured variables are read from the env struct passed as the first argument */
static int32_t NOINL v_closure_call_f(const struct v_cc_env *e, int32_t x) {
  return (int32_t)((uint32_t)x + (uint32_t)e->a + (uint32_t)e->b);
}

/* equiv: closure = env struct {a, b} + function pointer taking env*; called once per iteration */
static double NOINL op_v_closure_call(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_cc_env e = { 3, 5 }, *p = &e;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(p);
    s = (int32_t)((uint32_t)s + (uint32_t)v_closure_call_f(p, i));
  }
  return (double)s;
}

struct v_cnt_env { int32_t cnt; };

/* equiv: closure body that adds k to the captured counter in its env */
static void NOINL v_closure_counter_bump(struct v_cnt_env *e, int32_t k) {
  e->cnt = (int32_t)((uint32_t)e->cnt + (uint32_t)k);
}

/* equiv: env struct holding the captured counter; each call writes it through the env pointer */
static double NOINL op_v_closure_counter(long n) {
  int32_t N = (int32_t)n;
  struct v_cnt_env e = { 0 }, *p = &e;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    v_closure_counter_bump(p, i & 3);
  }
  return (double)p->cnt;
}

struct v_bind_obj { int32_t k; };
struct v_bound { int32_t (*fn)(const struct v_bind_obj *self, int32_t x); const struct v_bind_obj *self; };

/* equiv: function body taking the receiver explicitly as its first argument */
static int32_t NOINL v_bind_call_getk(const struct v_bind_obj *self, int32_t x) {
  return (int32_t)((uint32_t)self->k + (uint32_t)x);
}

/* equiv: bound function = (code pointer, bound receiver) pair; a call applies the receiver */
static double NOINL op_v_bind_call(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_bind_obj o = { 3 };
  struct v_bound f = { v_bind_call_getk, &o }, *fp = &f;
  OPAQUE_P(fp);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(fp);
    s = (int32_t)((uint32_t)s + (uint32_t)fp->fn(fp->self, i));
  }
  return (double)s;
}

struct v_fc_obj { int32_t k; };

/* equiv: function body with `this` passed as the first argument */
static int32_t NOINL v_fn_call_f(const struct v_fc_obj *self, int32_t x) {
  return (int32_t)((uint32_t)self->k * 2u + (uint32_t)x);
}

/* equiv: direct call with the receiver pointer passed explicitly (what f.call(o, x) compiles to) */
static double NOINL op_v_fn_call(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_fc_obj o = { 3 }, *p = &o;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(p);
    s = (int32_t)((uint32_t)s + (uint32_t)v_fn_call_f(p, i));
  }
  return (double)s;
}

struct v_fa_obj { int32_t k; };

/* equiv: function body with `this` and two arguments passed explicitly */
static int32_t NOINL v_fn_apply_f(const struct v_fa_obj *self, int32_t a, int32_t b) {
  return (int32_t)((uint32_t)self->k + (uint32_t)a + (uint32_t)b);
}

/* equiv: receiver pointer plus the argument array's two elements passed as scalars (apply spreads the array) */
static double NOINL op_v_fn_apply(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_fa_obj o = { 3 }, *p = &o;
  int32_t arr2[2] = { 0, 4 };
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(p);
    arr2[0] = i & 255;
    ESCAPE(arr2);
    s = (int32_t)((uint32_t)s + (uint32_t)v_fn_apply_f(p, arr2[0], arr2[1]));
  }
  return (double)s;
}

/* equiv: argv pointer plus argc; arguments.length is argc and arguments[0] is argv[0] */
static int32_t NOINL v_arguments_length_f(const int32_t *argv, int32_t argc) {
  return (int32_t)((uint32_t)argc + (uint32_t)argv[0]);
}

/* equiv: caller fills a two-slot argument block (arity 2, first arg i, second arg 7) and passes pointer+count */
static double NOINL op_v_arguments_length(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t argc = 2;
  OPAQUE_I(argc);
  int32_t av[2];
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    av[0] = i;
    av[1] = 7;
    s = (int32_t)((uint32_t)s + (uint32_t)v_arguments_length_f(av, argc));
  }
  return (double)s;
}

struct v_rt { int32_t state, acc, count; };

/* equiv: Richards-style task step: switch on the state field, update acc, advance state */
static int32_t NOINL v_richards_task_step(struct v_rt *self, int32_t x) {
  switch (self->state) {
    case 0: self->acc = (int32_t)((uint32_t)self->acc + (uint32_t)x); self->state = 1; break;
    case 1: self->acc = (int32_t)((uint32_t)self->acc ^ (uint32_t)x); self->state = 2; break;
    case 2: self->acc = (int32_t)((uint32_t)self->acc - (uint32_t)x); self->state = 3; break;
    default: self->acc = (int32_t)((uint32_t)self->acc + (uint32_t)x + (uint32_t)x); self->state = 0; break;
  }
  self->count = (int32_t)((uint32_t)self->count + 1u);
  return self->acc;
}

/* equiv: task struct (state, acc, count); one non-inlined step call per iteration, state read from memory each time */
static double NOINL op_v_richards_task(long n) {
  int32_t N = (int32_t)n;
  struct v_rt t = { 0, 0, 0 }, *q = &t;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(q);
    (void)v_richards_task_step(q, i);
  }
  return (double)(int32_t)((uint32_t)q->acc + (uint32_t)q->count);
}

struct v_db_var { int32_t value; };
struct v_db_hdr { const struct v_db_vt *vt; struct v_db_var *v1, *v2; };
struct v_db_vt { void (*execute)(const struct v_db_hdr *self); };

/* equiv: equality constraint body: v2.value = v1.value */
static void NOINL v_db_eq_execute(const struct v_db_hdr *self) {
  self->v2->value = self->v1->value;
}
/* equiv: scale constraint body: v2.value = v1.value * 2 + 1 */
static void NOINL v_db_scale_execute(const struct v_db_hdr *self) {
  self->v2->value = (int32_t)((uint32_t)self->v1->value + (uint32_t)self->v1->value + 1u);
}
/* equiv: xor constraint body: v2.value = v1.value ^ 5 */
static void NOINL v_db_xor_execute(const struct v_db_hdr *self) {
  self->v2->value = (int32_t)((uint32_t)self->v1->value ^ 5u);
}
static const struct v_db_vt v_db_vt_eq = { v_db_eq_execute };
static const struct v_db_vt v_db_vt_scale = { v_db_scale_execute };
static const struct v_db_vt v_db_vt_xor = { v_db_xor_execute };

/* equiv: three constraint objects sharing a header with a vtable pointer; execute() is an indirect call over two variable structs */
static double NOINL op_v_deltablue_constraint(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_db_var a = { 0 }, b = { 0 }, *pa = &a, *pb = &b;
  struct v_db_hdr cs[3] = {
    { &v_db_vt_eq, pa, pb }, { &v_db_vt_scale, pa, pb }, { &v_db_vt_xor, pa, pb }
  };
  OPAQUE_P(pa);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    ESCAPE(pa);
    ESCAPE(pb);
    pa->value = i;
    const struct v_db_hdr *c = &cs[i % 3];
    c->vt->execute(c);
    s = (int32_t)((uint32_t)s + (uint32_t)pb->value);
  }
  return (double)s;
}

/* ---- v_* group ob ---- */
/* ---------------- ob group: arrays and numeric kernels (NavierStokes, Crypto, Box2D, gaussian-blur, hash-map, sha256, float-mm, n-body, stanford-crypto) ---------------- */

/* equiv: stack array of 8 int32 with a length; push = store at len++, pop = load at --len */
static double NOINL op_v_array_push_pop(long n) {
  int32_t N = (int32_t)n, a = 0;
  int32_t arr[8] = {1, 2, 3, 4};
  int32_t len = 4;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    arr[len++] = (int32_t)((uint32_t)i ^ (uint32_t)a);
    int32_t popped = arr[--len];
    a = (int32_t)((uint32_t)a + (uint32_t)popped + (uint32_t)len);
  }
  return a;
}

static void NOINL v_ivec_push(struct ivec *v, int32_t x) {
  if (v->len == v->cap) {
    v->cap = v->cap ? v->cap * 2 : 4;
    v->d = realloc(v->d, v->cap * sizeof *v->d);
  }
  v->d[v->len++] = x;
}

/* equiv: growable int vector (malloc, doubling realloc from 4) created empty each outer iteration, 16 pushes, then freed */
static double NOINL op_v_array_push_grow(long n) {
  int32_t N = (int32_t)n, a = 0;
  int32_t outer = N >> 4;
  for (int32_t j = 0; j < outer; j++) {
    BODY_BARRIER();
    struct ivec v = { NULL, 0, 0 };
    for (int32_t k = 0; k < 16; k++) v_ivec_push(&v, (int32_t)((uint32_t)k + (uint32_t)j));
    a = (int32_t)((uint32_t)a + (uint32_t)v.d[15] + (uint32_t)v.len);
    free(v.d);
  }
  return a;
}

/* equiv: C array of row pointers (each row a separate malloc'd block), so grid[r][c] is two dependent loads as in JS */
static double NOINL op_v_array_2d(long n) {
  int32_t N = (int32_t)n, a = 0;
  int32_t *rows[16];
  for (int32_t r = 0; r < 16; r++) {
    rows[r] = malloc(16 * sizeof(int32_t));
    for (int32_t c = 0; c < 16; c++) rows[r][c] = r * 16 + c;
  }
  ESCAPE(rows);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)rows[(i >> 4) & 15][i & 15]);
  }
  for (int32_t r = 0; r < 16; r++) free(rows[r]);
  return a;
}

/* equiv: in-place Gauss-Seidel 4-neighbour stencil on a 18x18 double grid (NavierStokes lin_solve body), double arithmetic */
static double NOINL op_v_navier_lin_solve(long n) {
  int32_t N = (int32_t)n;
  double x[324], x0[324];
  for (int32_t k = 0; k < 324; k++) { x0[k] = (double)((k * 37) % 101) * 0.25; x[k] = 0.0; }
  ESCAPE(x); ESCAPE(x0);
  double a = 0.1, c = 1.0 + 4.0 * a;
  OPAQUE_D(a); OPAQUE_D(c);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t cell = i & 255;
    int32_t id = ((cell >> 4) + 1) * 18 + (cell & 15) + 1;
    double s = x[id - 18] + x[id + 18] + x[id - 1] + x[id + 1];
    x[id] = (x0[id] + a * s) / c;
  }
  double r = 0.0;
  for (int32_t k = 0; k < 324; k++) r = r + floor(x[k] * 4096.0);
  return r;
}

/* equiv: y[j] = a*x[j] + y[j] over two double arrays of 256 (daxpy) */
static double NOINL op_v_f64_axpy(long n) {
  int32_t N = (int32_t)n;
  double x[256], y[256];
  for (int32_t k = 0; k < 256; k++) { x[k] = (double)k * 0.25; y[k] = 1.0; }
  ESCAPE(x); ESCAPE(y);
  double a = 0.5;
  OPAQUE_D(a);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t j = i & 255;
    y[j] = a * x[j] + y[j];
  }
  double s = 0.0;
  for (int32_t k = 0; k < 256; k++) s = s + y[k];
  return s;
}

/* equiv: uint8_t buffer, load byte, xor with constant, store byte (in-place byte RMW) */
static double NOINL op_v_u8_bytes(long n) {
  int32_t N = (int32_t)n, s = 0;
  uint8_t b[256];
  for (int32_t k = 0; k < 256; k++) b[k] = (uint8_t)k;
  ESCAPE(b);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t j = (int32_t)(((uint32_t)i * 7u) & 255u);
    b[j] = (uint8_t)(b[j] ^ 0x5a);
  }
  for (int32_t k = 0; k < 256; k++) s = (int32_t)((uint32_t)s + (uint32_t)b[k]);
  return s;
}

/* equiv: BigInteger am3 inner loop (C from the Tom Wu / jsbn design: 28-bit digits, 14-bit halves), uint32 arithmetic (all values < 2^31) */
static uint32_t NOINL v_am3_step(const int32_t *a, int32_t *w, uint32_t x, uint32_t c, int32_t n) {
  uint32_t xl = x & 0x3fffu, xh = x >> 14;
  int32_t i = 0, j = 0;
  while (--n >= 0) {
    uint32_t l = (uint32_t)a[i] & 0x3fffu;
    uint32_t h = (uint32_t)a[i] >> 14;
    i++;
    uint32_t m = xh * l + h * xl;
    l = xl * l + ((m & 0x3fffu) << 14) + (uint32_t)w[j] + c;
    c = (l >> 28) + (m >> 14) + xh * h;
    w[j++] = (int32_t)(l & 0xfffffffu);
  }
  return c;
}

/* equiv: one bignum digit-row multiply-accumulate per call, 16 digits per call, n/16 calls */
static double NOINL op_v_crypto_am3(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t a[16], w[16];
  for (int32_t k = 0; k < 16; k++) {
    a[k] = (int32_t)(((uint32_t)k * 2654435u + 12345u) & 0xfffffffu);
    w[k] = (int32_t)(((uint32_t)k * 40503u + 7u) & 0xfffffffu);
  }
  ESCAPE(a); ESCAPE(w);
  int32_t outer = N >> 4;
  for (int32_t r = 0; r < outer; r++) {
    BODY_BARRIER();
    uint32_t x = ((uint32_t)r * 7919u + 3u) & 0xfffffffu;
    s = (int32_t)((uint32_t)s + v_am3_step(a, w, x, 0u, 16));
  }
  for (int32_t k = 0; k < 16; k++) s = (int32_t)((uint32_t)s + (uint32_t)w[k]);
  return s;
}

/* equiv: SHA-256 compression round body (Sigma1, Ch, Sigma0, Maj, rotr as shift-or) on uint32 locals */
static double NOINL op_v_sha_round(long n) {
  int32_t N = (int32_t)n;
  uint32_t a = 0x6a09e667u, b = 0xbb67ae85u, c = 0x3c6ef372u, d = 0xa54ff53au;
  uint32_t e = 0x510e527fu, f = 0x9b05688cu, g = 0x1f83d9abu, h = 0x5be0cd19u;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    uint32_t w = (uint32_t)i * 0x85ebca6bu + 0x27d4eb2fu;
    uint32_t S1 = ((e >> 6) | (e << 26)) ^ ((e >> 11) | (e << 21)) ^ ((e >> 25) | (e << 7));
    uint32_t ch = (e & f) ^ (~e & g);
    uint32_t t1 = h + S1 + ch + 0x428a2f98u + w;
    uint32_t S0 = ((a >> 2) | (a << 30)) ^ ((a >> 13) | (a << 19)) ^ ((a >> 22) | (a << 10));
    uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
    uint32_t t2 = S0 + maj;
    h = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
  }
  return (double)(int32_t)(a ^ b ^ c ^ d ^ e ^ f ^ g ^ h);
}

/* equiv: pairwise gravitational step of two point masses in doubles (sqrt via libm sqrt, IEEE-identical to JS Math.sqrt) */
static double NOINL op_v_nbody_step(long n) {
  int32_t N = (int32_t)n;
  double x1 = 1.0, y1 = 0.0, z1 = 0.0, vx1 = 0.0, vy1 = 0.5, vz1 = 0.0;
  double x2 = -1.0, y2 = 0.1, z2 = 0.0, vx2 = 0.0, vy2 = -0.5, vz2 = 0.0;
  double m1 = 1.0, m2 = 0.5, dt = 0.001, eps = 0.1;
  OPAQUE_D(m1); OPAQUE_D(m2); OPAQUE_D(dt); OPAQUE_D(eps);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    double dx = x1 - x2, dy = y1 - y2, dz = z1 - z2;
    double d2 = dx * dx + dy * dy + dz * dz + eps;
    double mag = dt / (d2 * sqrt(d2));
    vx1 = vx1 - dx * m2 * mag; vy1 = vy1 - dy * m2 * mag; vz1 = vz1 - dz * m2 * mag;
    vx2 = vx2 + dx * m1 * mag; vy2 = vy2 + dy * m1 * mag; vz2 = vz2 + dz * m1 * mag;
    x1 = x1 + dt * vx1; y1 = y1 + dt * vy1; z1 = z1 + dt * vz1;
    x2 = x2 + dt * vx2; y2 = y2 + dt * vy2; z2 = z2 + dt * vz2;
  }
  return floor((x1 + y1 + z1 + x2 + y2 + z2) * 1000.0);
}

/* equiv: 3x3 double GEMM accumulate C += A*B, fully unrolled inner dot product, n/9 calls */
static double NOINL op_v_float_mm(long n) {
  int32_t N = (int32_t)n;
  double A[9], B[9], C[9];
  for (int32_t k = 0; k < 9; k++) { A[k] = (double)(k + 1) * 0.5; B[k] = (double)(9 - k) * 0.25; C[k] = 0.0; }
  ESCAPE(A); ESCAPE(B); ESCAPE(C);
  int32_t iters = N / 9;
  for (int32_t i = 0; i < iters; i++) {
    BODY_BARRIER();
    for (int32_t r = 0; r < 3; r++)
      for (int32_t c = 0; c < 3; c++)
        C[r * 3 + c] += A[r * 3] * B[c] + A[r * 3 + 1] * B[3 + c] + A[r * 3 + 2] * B[6 + c];
  }
  double s = 0.0;
  for (int32_t k = 0; k < 9; k++) s = s + C[k];
  return s;
}

/* equiv: int32 index converted to double, scaled, mixed into a double accumulator */
static double NOINL op_v_int_to_double(long n) {
  int32_t N = (int32_t)n;
  double a = 0.25;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t k = i & 1023;
    a = a + ((double)k * 0.5 - (double)i * 0.25);
  }
  return a;
}

/* equiv: libm sin and cos of the same argument, each term floored to an integer (glibc sin/cos vs V8 fdlibm agree except at 1-ulp ties) */
static double NOINL op_v_math_sin_cos(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    double x = (double)i * 0.001;
    double q = floor((sin(x) + cos(x)) * 1024.0);
    a = (int32_t)((uint32_t)a + (uint32_t)(int32_t)q);
  }
  return a;
}

struct v_acc { int32_t s; };
/* equiv: forEach callback body, a NOINL function taking the captured accumulator pointer */
static void NOINL v_foreach_cb(struct v_acc *e, int32_t x) { e->s = (int32_t)((uint32_t)e->s + (uint32_t)x); }
/* equiv: forEach driver, calls the callback through a function pointer once per element */
static void NOINL v_foreach_run(const int32_t *arr, int32_t len, void (*cb)(struct v_acc *, int32_t), struct v_acc *e) {
  for (int32_t k = 0; k < len; k++) cb(e, arr[k]);
}

/* equiv: forEach over a 16-element C array through a callback pointer, n/16 calls */
static double NOINL op_v_array_foreach(long n) {
  int32_t N = (int32_t)n;
  int32_t arr[16];
  for (int32_t k = 0; k < 16; k++) arr[k] = k;
  ESCAPE(arr);
  struct v_acc acc = { 0 }, *p = &acc;
  int32_t outer = N >> 4;
  for (int32_t r = 0; r < outer; r++) {
    BODY_BARRIER();
    v_foreach_run(arr, 16, v_foreach_cb, p);
  }
  return acc.s;
}

/* equiv: map callback x -> x*2 as a NOINL function */
static int32_t NOINL v_dbl(int32_t x) { return (int32_t)((uint32_t)x * 2u); }
/* equiv: map driver writing into a caller-provided output block through a function pointer */
static void NOINL v_map_run(const int32_t *src, int32_t *dst, int32_t len, int32_t (*f)(int32_t)) {
  for (int32_t k = 0; k < len; k++) dst[k] = f(src[k]);
}

/* equiv: map into a bump-allocated output block (one arena for the op call, 16 ints per outer iteration), n/16 calls */
static double NOINL op_v_array_map(long n) {
  int32_t N = (int32_t)n;
  int32_t arr[16];
  for (int32_t k = 0; k < 16; k++) arr[k] = k;
  ESCAPE(arr);
  int32_t outer = N >> 4;
  int32_t *arena = malloc(sizeof(int32_t) * 16 * (size_t)(outer > 0 ? outer : 1));
  int32_t s = 0;
  for (int32_t r = 0; r < outer; r++) {
    BODY_BARRIER();
    int32_t *m = arena + 16 * r;
    v_map_run(arr, m, 16, v_dbl);
    s = (int32_t)((uint32_t)s + (uint32_t)m[r & 15]);
  }
  free(arena);
  return s;
}

/* equiv: reduce callback (acc + x) as a NOINL function */
static int32_t NOINL v_reduce_add(int32_t p, int32_t x) { return (int32_t)((uint32_t)p + (uint32_t)x); }
/* equiv: reduce driver, left fold through a function pointer */
static int32_t NOINL v_reduce_run(const int32_t *arr, int32_t len, int32_t init, int32_t (*f)(int32_t, int32_t)) {
  int32_t acc = init;
  for (int32_t k = 0; k < len; k++) acc = f(acc, arr[k]);
  return acc;
}

/* equiv: reduce sum over a 16-element C array, n/16 calls */
static double NOINL op_v_array_reduce(long n) {
  int32_t N = (int32_t)n;
  int32_t arr[16];
  for (int32_t k = 0; k < 16; k++) arr[k] = k * 3 + 1;
  ESCAPE(arr);
  int32_t s = 0;
  int32_t outer = N >> 4;
  for (int32_t r = 0; r < outer; r++) {
    BODY_BARRIER();
    int32_t *p = arr; OPAQUE_P(p); /* the call reads the array each time, as JS's reduce does */
    s = (int32_t)((uint32_t)s + (uint32_t)v_reduce_run(p, 16, 0, v_reduce_add));
  }
  return s;
}

/* equiv: linear scan of 16 int32, first match index or -1 */
static int32_t NOINL v_indexof16(const int32_t *a, int32_t key) {
  for (int32_t k = 0; k < 16; k++) if (a[k] == key) return k;
  return -1;
}

/* equiv: linear search of a 16-int C array for a key */
static double NOINL op_v_array_indexof(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t arr[16];
  for (int32_t k = 0; k < 16; k++) arr[k] = k * 3;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)((uint32_t)s + (uint32_t)v_indexof16(arr, i & 63));
  }
  return s;
}

/* equiv: fixed C int32[32] with the never-set slots holding 0 (a JS hole read through |0 is 0) */
static double NOINL op_v_array_holey(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t arr[32];
  for (int32_t k = 0; k < 32; k++) arr[k] = (k & 1) ? 0 : k;
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)((uint32_t)s + (uint32_t)arr[i & 31]);
  }
  return s;
}

/* equiv: array of 256 pointers to separately malloc'd struct obj (one heap object per element), read field v */
static double NOINL op_v_array_of_objects_field(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct obj *arr[256];
  for (int32_t k = 0; k < 256; k++) { arr[k] = malloc(sizeof *arr[k]); arr[k]->v = k; }
  ESCAPE(arr);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)((uint32_t)s + (uint32_t)arr[i & 255]->v);
  }
  for (int32_t k = 0; k < 256; k++) free(arr[k]);
  return s;
}

/* equiv: insertion sort of 16 int32 into ascending order, in place (chosen over qsort: no comparator call, 16 elements) */
static void NOINL v_insertion_sort16(int32_t *a) {
  for (int32_t i = 1; i < 16; i++) {
    int32_t x = a[i];
    int32_t j = i - 1;
    while (j >= 0 && a[j] > x) { a[j + 1] = a[j]; j--; }
    a[j + 1] = x;
  }
}

/* equiv: fill a 16-int C array, memcpy it into a fresh buffer, insertion-sort the copy, n/16 iterations */
static double NOINL op_v_typed_from_array_sort(long n) {
  int32_t N = (int32_t)n, s = 0;
  int32_t base[16];
  int32_t outer = N >> 4;
  for (int32_t r = 0; r < outer; r++) {
    BODY_BARRIER();
    for (int32_t k = 0; k < 16; k++) base[k] = (k * 73 + r * 29) & 255;
    int32_t t[16];
    memcpy(t, base, sizeof t);
    v_insertion_sort16(t);
    s = (int32_t)((uint32_t)s + (uint32_t)t[r & 15]);
  }
  return s;
}

/* equiv: 5-tap FIR over a float[256] ring; arithmetic in double then one rounding to float on store, matching JS Float32Array store */
static double NOINL op_v_gaussian_blur(long n) {
  int32_t N = (int32_t)n;
  float src[256], dst[256];
  for (int32_t k = 0; k < 256; k++) { src[k] = (float)((k * 37) & 255); dst[k] = 0.0f; }
  ESCAPE(src); ESCAPE(dst);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t j = i & 255;
    double v = 0.0625 * (double)src[(j + 254) & 255] + 0.25 * (double)src[(j + 255) & 255]
             + 0.375 * (double)src[j] + 0.25 * (double)src[(j + 1) & 255] + 0.0625 * (double)src[(j + 2) & 255];
    dst[j] = (float)v;
  }
  double s = 0.0;
  for (int32_t k = 0; k < 256; k++) s = s + (double)dst[k];
  return s;
}

/* ---- v_* group oc ---- */
/* ---------------- v_: foundational string / regexp / JSON constructs (group oc) ---------------- */

/* equiv: hash table with string keys (FNV-1a), preallocated; the dictionary lookup a JS object does */
struct v_slot { const char *k; int32_t v; };

static uint32_t NOINL v_fnv1a(const char *s) {
  uint32_t h = 2166136261u;
  for (; *s; s++) { h ^= (uint8_t)*s; h *= 16777619u; }
  return h;
}

static void NOINL v_dict_set(struct v_slot *t, const char *k, int32_t v) {
  uint32_t h = v_fnv1a(k) & 31u;
  while (t[h].k) h = (h + 1u) & 31u;
  t[h].k = k; t[h].v = v;
}

static int32_t NOINL v_dict_get(const struct v_slot *t, const char *k) {
  uint32_t h = v_fnv1a(k) & 31u;
  while (t[h].k && strcmp(t[h].k, k) != 0) h = (h + 1u) & 31u;
  return t[h].k ? t[h].v : -1;
}

/* equiv: strcmp-style length check + memcmp on two separately stored buffers */
static int32_t NOINL v_str_eq(const char *a, size_t la, const char *b, size_t lb) {
  return la == lb && memcmp(a, b, la) == 0;
}

/* equiv: one-pass matcher for /^[a-z]+\d+$/ (no backtracking needed: classes are disjoint) */
static int32_t NOINL v_rx_test(const char *s) {
  const char *p = s;
  if (!(*p >= 'a' && *p <= 'z')) return 0;
  while (*p >= 'a' && *p <= 'z') p++;
  if (!(*p >= '0' && *p <= '9')) return 0;
  while (*p >= '0' && *p <= '9') p++;
  return *p == '\0';
}

#define V_ISW(c) (((c) >= 'a' && (c) <= 'z') || ((c) >= 'A' && (c) <= 'Z') || ((c) >= '0' && (c) <= '9') || (c) == '_')
#define V_ISD(c) ((c) >= '0' && (c) <= '9')

/* equiv: unanchored search for (\w+)=(\d+) written out by hand; reports group 1 length and group 2 value */
static int32_t NOINL v_rx_exec(const char *s, size_t *g1len, int32_t *g2val) {
  for (const char *st = s; *st; st++) {
    const char *p = st;
    if (!V_ISW(*p)) continue;
    while (V_ISW(*p)) p++;
    if (*p != '=') continue;
    const char *eq = p;
    p++;
    if (!V_ISD(*p)) continue;
    int32_t v = 0;
    while (V_ISD(*p)) { v = v * 10 + (*p - '0'); p++; }
    *g1len = (size_t)(eq - st);
    *g2val = v;
    return 1;
  }
  return 0;
}

/* equiv: reads the next run of decimal digits from a pointer (hand parser for the fixed JSON shape) */
static int32_t NOINL v_json_int(const char **pp) {
  const char *p = *pp;
  while (*p && !V_ISD(*p)) p++;
  int32_t v = 0;
  while (V_ISD(*p)) { v = v * 10 + (*p - '0'); p++; }
  *pp = p;
  return v;
}

/* equiv: growable char buffer: append two bytes, double capacity on demand, length resets at 64 */
static double NOINL op_v_str_build(long n) {
  int32_t N = (int32_t)n;
  size_t len = 0, cap = 16, total = 0;
  char *buf = malloc(cap);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (len + 2 > cap) { cap *= 2; buf = realloc(buf, cap); }
    buf[len++] = 'a';
    buf[len++] = 'b';
    if (len >= 64) { total += len; len = 0; }
  }
  double r = (double)(total + len);
  free(buf);
  return r;
}

/* equiv: byte loop over a 64-byte source with range compares (a lexer's character classes) */
static double NOINL op_v_str_charcode_scan(long n) {
  int32_t N = (int32_t)n, id = 0, dg = 0, sp = 0, ot = 0;
  const char *src = "let x1 = foo(bar,42) + baz[7] * 3.5; return x1 >= 10 && yy || z;";
  OPAQUE_P(src);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    uint8_t c = (uint8_t)src[i & 63];
    if (c >= 97 && c <= 122) id++;
    else if (c >= 48 && c <= 57) dg++;
    else if (c == 32) sp++;
    else ot++;
  }
  return (double)(id + dg * 3 + sp * 5 + ot * 7);
}

/* equiv: write one byte into a 2-byte buffer and measure it with strlen */
static double NOINL op_v_str_fromcharcode(long n) {
  int32_t N = (int32_t)n, a = 0;
  char buf[2];
  buf[1] = '\0';
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    buf[0] = (char)(65 + (i & 15));
    ESCAPE(buf);
    a = (int32_t)((uint32_t)a + (uint32_t)strlen(buf));
  }
  return a;
}

/* equiv: memmem of a 3-byte needle in a 54-byte haystack; -1 when absent */
static double NOINL op_v_str_indexof(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *s = "the quick brown fox jumps over the lazy dog 0123456789";
  const char *nd = "dog";
  size_t slen = 54;
  OPAQUE_P(s); OPAQUE_P(nd); OPAQUE_I(slen);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const char *h = memmem(s, slen, nd, 3);
    int32_t idx = h ? (int32_t)(h - s) : -1;
    a = (int32_t)((uint32_t)a + (uint32_t)idx);
  }
  return a;
}

/* equiv: substring as pointer+length into the source (no copy), plus one byte load */
static double NOINL op_v_str_slice(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *s = "the quick brown fox jumps over the lazy dog 0123456789";
  size_t slen = 54;
  OPAQUE_P(s); OPAQUE_I(slen);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    size_t k = (size_t)(i & 15);
    size_t e = k + 8;
    if (e > slen) e = slen;
    const char *p = s + k;
    OPAQUE_P(p);
    size_t len = e - k;
    a = (int32_t)((uint32_t)a + (uint32_t)len + (uint32_t)(uint8_t)s[0]);
  }
  return a;
}

/* equiv: split into 4 (pointer,length) pairs on the stack, then join them with '-' into a reused buffer */
static double NOINL op_v_str_split_join(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *s = "alpha beta gamma delta";
  size_t slen = 22;
  OPAQUE_P(s); OPAQUE_I(slen);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const char *pp[4];
    size_t pl[4];
    size_t start = 0;
    int32_t k = 0;
    for (size_t j = 0; j <= slen; j++) {
      if (j == slen || s[j] == ' ') { pp[k] = s + start; pl[k] = j - start; k++; start = j + 1; }
    }
    char buf[32];
    size_t bl = 0;
    for (int32_t m = 0; m < k; m++) {
      if (m) buf[bl++] = '-';
      memcpy(buf + bl, pp[m], pl[m]);
      bl += pl[m];
    }
    ESCAPE(buf);
    a = (int32_t)((uint32_t)a + (uint32_t)bl + (uint32_t)k);
  }
  return a;
}

/* equiv: snprintf of "%d-%d" (the template literal with two ints) */
static double NOINL op_v_str_template(long n) {
  int32_t N = (int32_t)n, a = 0;
  char buf[32];
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int len = snprintf(buf, sizeof buf, "%d-%d", (int)(i & 255), (int)(i & 15));
    a = (int32_t)((uint32_t)a + (uint32_t)len);
  }
  return a;
}

/* equiv: strcmp(a, b) < 0 on two 8-byte strings differing at the last byte */
static double NOINL op_v_str_compare_lt(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *a = "abcdefgh", *b = "abcdefgi";
  OPAQUE_P(a); OPAQUE_P(b);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (strcmp(a, b) < 0) c = c + 1.0; }
  return c;
}

/* equiv: per-byte ASCII lowercase into a reused 8-byte buffer, then one byte load */
static double NOINL op_v_str_tolower(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *s = "ABCDEFGH";
  char buf[9];
  buf[8] = '\0';
  OPAQUE_P(s);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    for (int32_t k = 0; k < 8; k++) {
      char c = s[k];
      buf[k] = (c >= 'A' && c <= 'Z') ? (char)(c + 32) : c;
    }
    ESCAPE(buf);
    a = (int32_t)((uint32_t)a + (uint32_t)(uint8_t)buf[i & 7]);
  }
  return a;
}

/* equiv: two separately malloc'd 8-byte buffers with equal bytes; compare length then memcmp */
static double NOINL op_v_str_eq_dynamic(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *parts[4] = {"ab", "cd", "ef", "gh"};
  char *x = malloc(8), *y = malloc(8);
  for (int32_t k = 0; k < 4; k++) { memcpy(x + 2 * k, parts[k], 2); memcpy(y + 2 * k, parts[k], 2); }
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const char *px = x, *py = y;
    OPAQUE_P(px); OPAQUE_P(py);
    if (v_str_eq(px, 8, py, 8)) c = c + 1.0;
  }
  free(x);
  free(y);
  return c;
}

/* equiv: snprintf("%d") of a value in 0..1023 and its length */
static double NOINL op_v_num_tostring(long n) {
  int32_t N = (int32_t)n, a = 0;
  char buf[16];
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int len = snprintf(buf, sizeof buf, "%d", (int)(i & 1023));
    a = (int32_t)((uint32_t)a + (uint32_t)len);
  }
  return a;
}

/* equiv: strtol base 10 over a 16-entry table of C strings */
static double NOINL op_v_parse_int(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *t[16] = {"12", "7", "301", "45", "9", "1024", "88", "3", "560", "21", "6", "777", "42", "100", "5", "2048"};
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)strtol(t[i & 15], NULL, 10));
  }
  return a;
}

/* equiv: strtod over a 16-entry table of C strings (exact binary fractions, so sums are exact) */
static double NOINL op_v_parse_float(long n) {
  int32_t N = (int32_t)n;
  double a = 0;
  const char *t[16] = {"3.25", "0.5", "12.75", "100.125", "7.0", "0.25", "2.5", "64.5", "1024.0625", "9.75", "0.125", "5", "33.5", "18.25", "2.0", "0.75"};
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = a + strtod(t[i & 15], NULL);
  }
  return a;
}

/* equiv: hand matcher v_rx_test on a 16-entry table of C strings */
static double NOINL op_v_regexp_test(long n) {
  int32_t N = (int32_t)n;
  double c = 0;
  const char *t[16] = {"abc123", "xyz", "a1", "hello world", "foo42", "BAR7", "q", "z99x", "pq0", "12ab", "mno789", "k", "ab-12", "go0go", "x_1", "tuv2024"};
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (v_rx_test(t[i & 15])) c = c + 1.0;
  }
  return c;
}

/* equiv: hand matcher v_rx_exec returning the two capture groups in out-parameters */
static double NOINL op_v_regexp_exec_groups(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *t[8] = {"key=12", "alpha=7", "x=100", "name=3", "val=4096", "k=0", "abc=99", "n=5"};
  ESCAPE(t);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    size_t g1 = 0;
    int32_t g2 = 0;
    v_rx_exec(t[i & 7], &g1, &g2);
    a = (int32_t)((uint32_t)a + (uint32_t)g1 + (uint32_t)g2);
  }
  return a;
}

/* equiv: copy the 43-byte string, mapping 'o' to '0' into a reused buffer, then strlen */
static double NOINL op_v_str_replace_regexp(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *s = "the quick brown fox jumps over the lazy dog";
  size_t sl = 43;
  char buf[64];
  OPAQUE_P(s); OPAQUE_I(sl);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    for (size_t k = 0; k < sl; k++) buf[k] = (s[k] == 'o') ? '0' : s[k];
    buf[sl] = '\0';
    ESCAPE(buf);
    a = (int32_t)((uint32_t)a + (uint32_t)strlen(buf));
  }
  return a;
}

/* equiv: snprintf of the fixed JSON shape with the changing int (its text length) */
static double NOINL op_v_json_stringify(long n) {
  int32_t N = (int32_t)n, a = 0;
  char buf[64];
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int len = snprintf(buf, sizeof buf, "{\"a\":%d,\"b\":\"str\",\"c\":[1,2]}", (int)(i & 1023));
    a = (int32_t)((uint32_t)a + (uint32_t)len);
  }
  return a;
}

/* equiv: hand parser for the fixed shape: reads the digit runs in order (a, b[0], b[1], b[2]) and sums them */
static double NOINL op_v_json_parse(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *src = "{\"a\":12,\"b\":[1,2,3]}";
  OPAQUE_P(src);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const char *p = src;
    int32_t av = v_json_int(&p);
    int32_t b0 = v_json_int(&p);
    int32_t b1 = v_json_int(&p);
    int32_t b2 = v_json_int(&p);
    a = (int32_t)((uint32_t)a + (uint32_t)av + (uint32_t)b0 + (uint32_t)b1 + (uint32_t)b2);
  }
  return a;
}

/* equiv: open-addressing string hash table (FNV-1a, 32 slots, linear probe); lookup hashes the key each time */
static double NOINL op_v_str_key_dict(long n) {
  int32_t N = (int32_t)n, a = 0;
  const char *keys[16] = {"red", "green", "blue", "cyan", "gold", "pink", "navy", "teal", "lime", "gray", "rose", "sand", "plum", "jade", "ruby", "mint"};
  struct v_slot tab[32];
  memset(tab, 0, sizeof tab);
  for (int32_t j = 0; j < 16; j++) v_dict_set(tab, keys[j], j * 3 + 1);
  ESCAPE(keys);
  ESCAPE(tab);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int32_t)((uint32_t)a + (uint32_t)v_dict_get(tab, keys[i & 15]));
  }
  return a;
}

/* equiv: memcpy of two short pieces and a '-' into a reused buffer; the length is the sum */
static double NOINL op_v_str_concat3(long n) {
  int32_t N = (int32_t)n;
  double s = 0;
  const char *P[4] = {"ab", "cde", "f", "ghij"};
  size_t PL[4] = {2, 3, 1, 4};
  ESCAPE(P);
  char buf[16];
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    size_t lx = PL[i & 3], ly = PL[(i >> 2) & 3];
    memcpy(buf, P[i & 3], lx);
    buf[lx] = '-';
    memcpy(buf + lx + 1, P[(i >> 2) & 3], ly);
    size_t tl = lx + 1 + ly;
    ESCAPE(buf);
    s = s + (double)tl;
  }
  return s;
}

/* ---- v_* group od ---- */

/* ---------------- od: maps/sets and control flow (v_ ops) ---------------- */

/* equiv: FNV-1a 32-bit string hash; the result is cached per key, like V8's string hash field */
static uint32_t v_fnv1a_len(const char *s, size_t len) {
  uint32_t h = 2166136261u;
  for (size_t k = 0; k < len; k++) { h ^= (unsigned char)s[k]; h *= 16777619u; }
  return h;
}
struct v_skey { const char *s; size_t len; uint32_t hash; };
struct v_pt2 { int32_t x, y; };

/* equiv: preallocated open-addressing int->int hash table, linear probe, multiplicative hash (Map.get) */
static int32_t v_mg_keys[512], v_mg_vals[512];
static int v_mg_probe(int32_t key) { uint32_t h = (uint32_t)key * 2654435769u >> 23; while (v_mg_keys[h] != key && v_mg_keys[h] != -1) h = (h + 1) & 511; return (int)h; }
static double NOINL op_v_map_get_int(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int j = 0; j < 512; j++) v_mg_keys[j] = -1;
  for (int32_t j = 0; j < 256; j++) { int h = v_mg_probe(j); v_mg_keys[h] = j; v_mg_vals[h] = j * 3; }
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); int h = v_mg_probe(i & 255); a = (int32_t)((uint32_t)a + (uint32_t)v_mg_vals[h]); }
  return (double)a;
}

/* equiv: open-addressing int set with backward-shift deletion (no tombstones), preallocated (Map.set + Map.delete) */
static int32_t v_md_keys[512], v_md_vals[512];
static uint32_t v_md_home(int32_t key) { return ((uint32_t)key * 2654435769u) >> 23; }
static int v_md_find(int32_t key) { uint32_t h = v_md_home(key); while (v_md_keys[h] != key) { if (v_md_keys[h] == -1) return -1; h = (h + 1) & 511; } return (int)h; }
static void NOINL v_md_set(int32_t key, int32_t val) {
  int s = v_md_find(key);
  if (s >= 0) { v_md_vals[s] = val; return; }
  uint32_t h = v_md_home(key);
  while (v_md_keys[h] != -1) h = (h + 1) & 511;
  v_md_keys[h] = key; v_md_vals[h] = val;
}
static int NOINL v_md_delete(int32_t key) {
  int i = v_md_find(key), j;
  if (i < 0) return 0;
  j = i;
  for (;;) {
    j = (j + 1) & 511;
    if (v_md_keys[j] == -1) break;
    int k = (int)v_md_home(v_md_keys[j]);
    int skip = (i <= j) ? (i < k && k <= j) : (i < k || k <= j);
    if (!skip) { v_md_keys[i] = v_md_keys[j]; v_md_vals[i] = v_md_vals[j]; v_md_keys[j] = -1; i = j; }
  }
  v_md_keys[i] = -1;
  return 1;
}
static double NOINL op_v_map_set_delete(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int j = 0; j < 512; j++) v_md_keys[j] = -1;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); int32_t k = i & 255; v_md_set(k, i); a = (int32_t)((uint32_t)a + (uint32_t)v_md_delete(k)); }
  return (double)a;
}

/* equiv: open-addressing string->int table, FNV-1a hashes precomputed, compare hash+len+memcmp (Map.get with string keys) */
static const char *const v_mgs_names[16] = {"alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi"};
static const struct v_skey *v_mgs_tab[32];
static int32_t v_mgs_val[32];
static void v_mgs_set(const struct v_skey *k, int32_t val) { uint32_t h = k->hash & 31u; while (v_mgs_tab[h]) h = (h + 1) & 31u; v_mgs_tab[h] = k; v_mgs_val[h] = val; }
static int32_t NOINL v_mgs_get(const struct v_skey *k) {
  uint32_t h = k->hash & 31u;
  while (v_mgs_tab[h]) {
    const struct v_skey *e = v_mgs_tab[h];
    if (e->hash == k->hash && e->len == k->len && memcmp(e->s, k->s, k->len) == 0) return v_mgs_val[h];
    h = (h + 1) & 31u;
  }
  return 0;
}
static double NOINL op_v_map_get_str(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct v_skey keys[16];
  for (int32_t j = 0; j < 16; j++) { keys[j].s = v_mgs_names[j]; keys[j].len = strlen(keys[j].s); keys[j].hash = v_fnv1a_len(keys[j].s, keys[j].len); }
  memset(v_mgs_tab, 0, sizeof v_mgs_tab);
  for (int32_t j = 0; j < 16; j++) v_mgs_set(&keys[j], j * 5 + 1);
  ESCAPE(keys);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)v_mgs_get(&keys[i & 15])); }
  return (double)a;
}

/* equiv: membership in a preallocated int hash set (Set.has) */
static int32_t v_sh_keys[512];
static int v_sh_probe(int32_t key) { uint32_t h = (uint32_t)key * 2654435769u >> 23; while (v_sh_keys[h] != key && v_sh_keys[h] != -1) h = (h + 1) & 511; return (int)h; }
static double NOINL op_v_set_has(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int j = 0; j < 512; j++) v_sh_keys[j] = -1;
  for (int32_t j = 0; j < 256; j++) v_sh_keys[v_sh_probe(j * 3)] = j * 3;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); if (v_sh_keys[v_sh_probe(i & 1023)] != -1) a = (int32_t)((uint32_t)a + 1u); }
  return (double)a;
}

/* equiv: walk a dense insertion-ordered entry array (the Map's backing store), key and value per entry */
static int32_t v_mi_k[16], v_mi_v[16];
static double NOINL op_v_map_iterate(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t j = 0; j < 16; j++) { v_mi_k[j] = j; v_mi_v[j] = j + 1; }
  ESCAPE(v_mi_k); ESCAPE(v_mi_v);
  for (int32_t i = 0; i < (N >> 4); i++) {
    BODY_BARRIER();
    for (int32_t j = 0; j < 16; j++) a = (int32_t)((uint32_t)a + (uint32_t)v_mi_k[j] + (uint32_t)v_mi_v[j]);
  }
  return (double)a;
}

/* equiv: fixed 4-field record; Object.keys copies the key list into a fresh array (malloc'd, freed: it escapes the step) */
struct v_obj4 { struct v_skey k[4]; int32_t v[4]; };
static int32_t NOINL v_keys_of(const struct v_obj4 *o, struct v_skey *out) { for (int j = 0; j < 4; j++) out[j] = o->k[j]; return 4; }
static double NOINL op_v_obj_keys_loop(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct v_obj4 o = { { {"alpha", 5, 0}, {"beta", 4, 0}, {"gamma", 5, 0}, {"delta", 5, 0} }, {1, 2, 3, 4} }, *p = &o;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct v_skey *ks = malloc(4 * sizeof *ks);
    int32_t cnt = v_keys_of(p, ks);
    for (int32_t j = 0; j < cnt; j++) a = (int32_t)((uint32_t)a + (uint32_t)ks[j].len);
    ESCAPE(ks); free(ks);
  }
  return (double)a;
}

/* equiv: fixed record walked field by field in declaration order (for..in over 4 keys: key length + value) */
static double NOINL op_v_for_in(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct v_obj4 o = { { {"alpha", 5, 0}, {"beta", 4, 0}, {"gamma", 5, 0}, {"delta", 5, 0} }, {1, 2, 3, 4} }, *p = &o;
  OPAQUE_P(p);
  for (int32_t i = 0; i < (N >> 2); i++) {
    BODY_BARRIER();
    for (int32_t j = 0; j < 4; j++) a = (int32_t)((uint32_t)a + (uint32_t)p->k[j].len + (uint32_t)p->v[j]);
  }
  return (double)a;
}

/* equiv: index loop over a 16-element int array (for..of over an array) */
static double NOINL op_v_for_of_array(long n) {
  int32_t N = (int32_t)n, a = 0, arr[16];
  for (int32_t j = 0; j < 16; j++) arr[j] = j * 7 + 1;
  int32_t *p = arr;
  OPAQUE_P(p);
  for (int32_t i = 0; i < (N >> 4); i++) {
    BODY_BARRIER();
    for (int32_t j = 0; j < 16; j++) a = (int32_t)((uint32_t)a + (uint32_t)p[j]);
  }
  return (double)a;
}

/* equiv: generator state struct; next() returns the saved counter then advances it */
struct v_gen { int32_t x; };
static int32_t NOINL v_gen_next(struct v_gen *g) { int32_t r = g->x; g->x = (int32_t)((uint32_t)g->x + 3u); return r; }
static double NOINL op_v_generator_next(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct v_gen g = {0}, *p = &g;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)v_gen_next(p)); }
  return (double)a;
}

/* equiv: straight-line code; a try block that never throws costs nothing */
static double NOINL op_v_try_nothrow(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)(i ^ 5)); }
  return (double)a;
}

/* equiv: setjmp/longjmp: the helper longjmps to the setjmp in the loop every 16th call (throw/catch) */
static jmp_buf v_tc_env;
static int32_t NOINL v_throw_helper(int32_t i) { if ((i & 15) == 0) longjmp(v_tc_env, 1); return i; }
static double NOINL op_v_throw_catch(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (setjmp(v_tc_env) == 0) a = (int32_t)((uint32_t)a + (uint32_t)v_throw_helper(i));
    else a = (int32_t)((uint32_t)a + 1u);
  }
  return (double)a;
}

/* equiv: call with the three elements of the array passed as separate arguments */
static int32_t NOINL v_spread_f(int32_t x, int32_t y, int32_t z) { return (int32_t)((uint32_t)x + (uint32_t)y + (uint32_t)z); }
static double NOINL op_v_spread_call(long n) {
  int32_t N = (int32_t)n, a = 0, arr3[3] = {0, 2, 3};
  int32_t *p = arr3;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); p[0] = i; a = (int32_t)((uint32_t)a + (uint32_t)v_spread_f(p[0], p[1], p[2])); }
  return (double)a;
}

/* equiv: variadic callee gets pointer + count; the caller fills a stack array (no heap array) */
static int32_t NOINL v_rest_g(int32_t cnt, const int32_t *r) { return (int32_t)((uint32_t)cnt + (uint32_t)r[0]); }
static double NOINL op_v_rest_args(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); int32_t r[3] = {i, 2, 3}; a = (int32_t)((uint32_t)a + (uint32_t)v_rest_g(3, r)); }
  return (double)a;
}

/* equiv: swap through temporaries (the array temporary is a register pair) */
static double NOINL op_v_destructure_array(long n) {
  int32_t N = (int32_t)n, a = 0, b = 1;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t na = b, nb = (int32_t)((uint32_t)a + (uint32_t)i);
    a = na; b = nb;
  }
  return (double)(int32_t)((uint32_t)a + (uint32_t)b);
}

/* equiv: read two fields of a fixed record (object destructuring is two field loads) */
static double NOINL op_v_destructure_obj(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct v_pt2 o = {3, 4}, *p = &o;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) { BODY_BARRIER(); a = (int32_t)((uint32_t)a + (uint32_t)p->x + (uint32_t)p->y + (uint32_t)(i & 1)); }
  return (double)a;
}

/* equiv: nullable pointer chain; a null link yields the default (1) with no load */
struct v_oc_p { int32_t q; };
struct v_oc_o { const struct v_oc_p *p; };
static double NOINL op_v_optional_chain(long n) {
  int32_t N = (int32_t)n, a = 0;
  struct v_oc_p pa = {5}, pc = {9};
  struct v_oc_o o0 = {&pa}, o2 = {NULL}, o3 = {&pc};
  const struct v_oc_o *objs[4] = {&o0, NULL, &o2, &o3}, **q = objs;
  OPAQUE_P(q);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    const struct v_oc_o *o = q[i & 3];
    int32_t v = (o && o->p) ? o->p->q : 1;
    a = (int32_t)((uint32_t)a + (uint32_t)v);
  }
  return (double)a;
}

/* equiv: nullable pointer: NULL means "use the default" (a ?? b) */
static double NOINL op_v_nullish(long n) {
  int32_t N = (int32_t)n, a = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t cell = i;
    const int32_t *x = (i & 1) ? NULL : &cell;
    a = (int32_t)((uint32_t)a + (uint32_t)(x ? *x : 3));
  }
  return (double)a;
}

/* equiv: plain (unbalanced, unsplayed) binary search tree of 256 struct nodes; find walks left/right */
struct v_sp { int32_t key; struct v_sp *left, *right; };
static void NOINL v_sp_insert(struct v_sp *root, struct v_sp *node) {
  struct v_sp *cur = root;
  for (;;) {
    if (node->key < cur->key) { if (!cur->left) { cur->left = node; return; } cur = cur->left; }
    else { if (!cur->right) { cur->right = node; return; } cur = cur->right; }
  }
}
static double NOINL op_v_splay_insert_find(long n) {
  int32_t N = (int32_t)n, a = 0, want = 0;
  struct v_sp pool[256];
  for (int32_t j = 0; j < 256; j++) { pool[j].key = (j * 97) & 255; pool[j].left = NULL; pool[j].right = NULL; }
  struct v_sp *root = &pool[0];
  for (int32_t j = 1; j < 256; j++) v_sp_insert(root, &pool[j]);
  ESCAPE(pool);
  struct v_sp *cur = root;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (cur->key == want) { a = (int32_t)((uint32_t)a + (uint32_t)cur->key); want = (want + 37) & 255; cur = root; }
    else cur = want < cur->key ? cur->left : cur->right;
  }
  return (double)a;
}

/* equiv: array-backed binary min-heap, sift-up / sift-down on a 64-slot int array */
static void NOINL v_heap_push(int32_t *h, int32_t size, int32_t v) {
  int32_t k = size;
  h[k] = v;
  while (k > 0) { int32_t p = (k - 1) >> 1; if (h[p] <= h[k]) break; int32_t t = h[p]; h[p] = h[k]; h[k] = t; k = p; }
}
static int32_t NOINL v_heap_pop(int32_t *h, int32_t size) {
  int32_t top = h[0], m = size - 1, k = 0;
  h[0] = h[m];
  for (;;) {
    int32_t l = 2 * k + 1, c = l;
    if (l >= m) break;
    if (l + 1 < m && h[l + 1] < h[l]) c = l + 1;
    if (h[k] <= h[c]) break;
    int32_t t = h[k]; h[k] = h[c]; h[c] = t; k = c;
  }
  return top;
}
static double NOINL op_v_astar_heap(long n) {
  int32_t N = (int32_t)n, size = 0, a = 0, h[64];
  for (int32_t j = 0; j < 64; j++) h[j] = 0;
  int32_t *p = h;
  OPAQUE_P(p);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    if (size == 0 || (size < 60 && (i % 3) != 2)) { v_heap_push(p, size, (i * 73 + 17) & 4095); size++; }
    else { a = (int32_t)((uint32_t)a + (uint32_t)v_heap_pop(p, size)); size--; }
  }
  return (double)a;
}

/* equiv: string switch as a strcmp chain returning the case index, then an int switch */
static const char *const v_sw_names[8] = {"add", "sub", "mul", "div", "mod", "and", "or", "xor"};
static int NOINL v_sw_match(const char *s) {
  if (!strcmp(s, "add")) return 0;
  if (!strcmp(s, "sub")) return 1;
  if (!strcmp(s, "mul")) return 2;
  if (!strcmp(s, "div")) return 3;
  if (!strcmp(s, "mod")) return 4;
  if (!strcmp(s, "and")) return 5;
  if (!strcmp(s, "or")) return 6;
  if (!strcmp(s, "xor")) return 7;
  return -1;
}
static double NOINL op_v_switch_string(long n) {
  int32_t N = (int32_t)n, acc = 1;
  const char *const *tbl = v_sw_names;
  OPAQUE_P(tbl);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    switch (v_sw_match(tbl[i & 7])) {
      case 0: acc = (int32_t)((uint32_t)acc + 1u); break;
      case 1: acc = (int32_t)((uint32_t)acc - 3u); break;
      case 2: acc = (int32_t)((uint32_t)acc * 3u); break;
      case 3: acc = acc / 2; break;
      case 4: acc = acc % 1009; break;
      case 5: acc = acc & 0xffff; break;
      case 6: acc = acc | 0x55; break;
      case 7: acc = acc ^ 0x5a5a; break;
    }
  }
  return (double)acc;
}

/* ---- v_* group oe ---- */
/* ---------------- oe group: allocation, GC churn and remaining foundations (V8 v7 suite, JetStream 3.0) ----------------
 * Fragment: paste after the macros of ops.c (NOINL, BODY_BARRIER, OPAQUE_P, ESCAPE; stdint, stdio, stdlib, string, math).
 * Object allocation is bump allocation from v_arena_buf, reset at the top of each iteration unless noted. */

static char v_arena_buf[4096] __attribute__((aligned(16)));
static size_t v_arena_top;
/* equiv: bump allocation from a preallocated arena (the JS object dies at the end of the iteration) */
static void *NOINL v_arena_alloc(size_t sz) { void *p = v_arena_buf + v_arena_top; v_arena_top += (sz + 15) & ~(size_t)15; return p; }

/* ---- v_alloc_small ---- */
struct v_small { int32_t v; };
/* equiv: a fresh 1-field object per iteration, bump-allocated from the arena and dead at iteration end */
static double NOINL op_v_alloc_small(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    v_arena_top = 0;
    struct v_small *o = v_arena_alloc(sizeof *o);
    o->v = i;
    s = (int32_t)((uint32_t)s + (uint32_t)o->v);
  }
  return s;
}

/* ---- v_alloc_array4 ---- */
/* equiv: a 4-element int array literal per iteration, bump-allocated from the arena */
static double NOINL op_v_alloc_array4(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    v_arena_top = 0;
    int32_t *arr = v_arena_alloc(4 * sizeof *arr);
    arr[0] = i; arr[1] = i + 1; arr[2] = i + 2; arr[3] = i + 3;
    s = (int32_t)((uint32_t)s + (uint32_t)arr[0] + (uint32_t)arr[1] + (uint32_t)arr[2] + (uint32_t)arr[3]);
  }
  return s;
}

/* ---- v_alloc_closure ---- */
struct v_clo_env { int32_t i, k; };
/* equiv: closure body; the captured variables are read through the env pointer */
static int32_t NOINL v_alloc_closure_fn(const struct v_clo_env *e, int32_t x) { return (int32_t)((uint32_t)x + (uint32_t)e->i + (uint32_t)e->k); }
/* equiv: a closure per iteration: env struct {i, k} plus a function taking env*, called once */
static double NOINL op_v_alloc_closure(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    v_arena_top = 0;
    struct v_clo_env *e = v_arena_alloc(sizeof *e); /* equiv: closure creation = env struct + fn pointer */
    e->i = i; e->k = i & 7;
    s = (int32_t)((uint32_t)s + (uint32_t)v_alloc_closure_fn(e, i & 7));
  }
  return s;
}

/* ---- v_alloc_retained ---- */
struct v_ret { int32_t a, b; };
static struct v_ret *v_alloc_retained_ring[1024];
/* equiv: malloc the new object into its ring slot and free the object it evicts (1024 objects stay live) */
static void NOINL v_alloc_retained_put(int32_t slot, int32_t a, int32_t b) {
  struct v_ret *o = malloc(sizeof *o);
  o->a = a; o->b = b;
  free(v_alloc_retained_ring[slot]);
  v_alloc_retained_ring[slot] = o;
}
/* equiv: malloc/free ring of 1024 live objects; the object in a slot is freed when it is evicted */
static double NOINL op_v_alloc_retained(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t j = 0; j < 1024; j++) v_alloc_retained_ring[j] = calloc(1, sizeof(struct v_ret));
  ESCAPE(v_alloc_retained_ring);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    v_alloc_retained_put(i & 1023, i, (int32_t)((uint32_t)i + 1u));
    s = (int32_t)((uint32_t)s + (uint32_t)v_alloc_retained_ring[(i + 512) & 1023]->b);
  }
  for (int32_t j = 0; j < 1024; j++) { free(v_alloc_retained_ring[j]); v_alloc_retained_ring[j] = NULL; }
  return s;
}

/* ---- v_cons_list (EarleyBoyer-style cons cells) ---- */
struct v_cell { int32_t car; struct v_cell *cdr; };
/* equiv: bump-allocate a cons cell from the arena and link it to cdr */
static struct v_cell *NOINL v_cons_list_cons(int32_t car, struct v_cell *cdr) {
  struct v_cell *c = v_arena_alloc(sizeof *c);
  c->car = car; c->cdr = cdr;
  return c;
}
/* equiv: 16 bump-allocated cons cells per outer iteration, linked, then walked and summed */
static double NOINL op_v_cons_list(long n) {
  int32_t outer = (int32_t)(n >> 4), s = 0;
  for (int32_t i = 0; i < outer; i++) {
    BODY_BARRIER();
    v_arena_top = 0;
    struct v_cell *l = NULL;
    for (int32_t j = 0; j < 16; j++) l = v_cons_list_cons(i + j, l);
    for (struct v_cell *p = l; p != NULL; p = p->cdr) s = (int32_t)((uint32_t)s + (uint32_t)p->car);
  }
  return s;
}

/* ---- v_vec3_ops (RayTrace-style Vector) ---- */
struct v_vec3 { double x, y, z; };
/* equiv: vector add returning a new value by struct return */
static struct v_vec3 NOINL v_vec3_ops_add(struct v_vec3 a, struct v_vec3 b) { struct v_vec3 r = {a.x + b.x, a.y + b.y, a.z + b.z}; return r; }
/* equiv: vector scale returning a new value by struct return */
static struct v_vec3 NOINL v_vec3_ops_scale(struct v_vec3 a, double k) { struct v_vec3 r = {a.x * k, a.y * k, a.z * k}; return r; }
/* equiv: dot product as a plain function of two structs */
static double NOINL v_vec3_ops_dot(struct v_vec3 a, struct v_vec3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
/* equiv: Vector struct passed and returned by value, one new vector per add and per scale */
static double NOINL op_v_vec3_ops(long n) {
  int32_t N = (int32_t)n;
  double s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct v_vec3 a = {(double)i, 1.0, 2.0};
    struct v_vec3 b = v_vec3_ops_scale(v_vec3_ops_add(a, (struct v_vec3){1.0, 2.0, 3.0}), 0.5);
    s += v_vec3_ops_dot(a, b);
  }
  return s;
}

/* ---- v_box2d_vec2 (mutable Vec2 updated in place by methods) ---- */
struct v_v2 { double x, y; };
/* equiv: method that updates the receiver's fields in place through its pointer */
static void NOINL v_box2d_vec2_add_scaled(struct v_v2 *p, const struct v_v2 *o, double k) { p->x += o->x * k; p->y += o->y * k; }
/* equiv: method that overwrites the receiver's fields in place */
static void NOINL v_box2d_vec2_set(struct v_v2 *p, double x, double y) { p->x = x; p->y = y; }
/* equiv: Vec2 struct updated in place through pointer-taking methods */
static double NOINL op_v_box2d_vec2(long n) {
  int32_t N = (int32_t)n;
  double s = 0;
  struct v_v2 p = {0, 0}, vel = {1.5, -0.25}, *pp = &p, *vp = &vel;
  OPAQUE_P(pp); OPAQUE_P(vp);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    v_box2d_vec2_add_scaled(pp, vp, 0.5);
    if (pp->x > 4096) v_box2d_vec2_set(pp, 0, pp->y);
    s += pp->x + pp->y;
  }
  return s;
}

/* ---- v_class_hierarchy_ctor (C extends B extends A) ---- */
struct v_hA { int32_t a; };
struct v_hB { struct v_hA base; int32_t b; };
struct v_hC { struct v_hB base; int32_t c; };
/* equiv: A's constructor, sets its one field */
static void NOINL v_class_hierarchy_ctor_A(struct v_hA *o, int32_t i) { o->a = i; }
/* equiv: B's constructor: super(i) runs A's body on the embedded base, then B's field */
static void NOINL v_class_hierarchy_ctor_B(struct v_hB *o, int32_t i) { v_class_hierarchy_ctor_A(&o->base, i); o->b = (int32_t)((uint32_t)i + 1u); }
/* equiv: C's constructor: super(i) runs B's body on the embedded base, then C's field */
static void NOINL v_class_hierarchy_ctor_C(struct v_hC *o, int32_t i) { v_class_hierarchy_ctor_B(&o->base, i); o->c = (int32_t)((uint32_t)i + 2u); }
/* equiv: C contains B contains A as nested structs, built by chained constructor calls */
static double NOINL op_v_class_hierarchy_ctor(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    v_arena_top = 0;
    struct v_hC *o = v_arena_alloc(sizeof *o);
    v_class_hierarchy_ctor_C(o, i);
    s = (int32_t)((uint32_t)s + (uint32_t)o->base.base.a + (uint32_t)o->base.b + (uint32_t)o->c);
  }
  return s;
}

/* ---- v_array_literal_objects (array of 4 small object literals per outer iteration) ---- */
struct v_lit { int32_t x; };
/* equiv: array of 4 pointers to small structs, bump-allocated per outer iteration */
static double NOINL op_v_array_literal_objects(long n) {
  int32_t outer = (int32_t)(n >> 2), s = 0;
  for (int32_t i = 0; i < outer; i++) {
    BODY_BARRIER();
    v_arena_top = 0;
    struct v_lit **arr = v_arena_alloc(4 * sizeof *arr); /* equiv: array literal = 4 pointers */
    for (int32_t j = 0; j < 4; j++) {
      struct v_lit *o = v_arena_alloc(sizeof *o);
      o->x = i + j;
      arr[j] = o;
    }
    for (int32_t j = 0; j < 4; j++) s = (int32_t)((uint32_t)s + (uint32_t)arr[j]->x);
  }
  return s;
}

/* ---- v_object_assign / v_object_spread (three-field object, copied) ---- */
struct v_abc { int32_t a, b, c; };
struct v_abcd { int32_t a, b, c, d; };
/* equiv: Object.assign({}, src): copy each own field of src into the fresh target */
static void NOINL v_object_assign_copy(struct v_abc *dst, const struct v_abc *src) { dst->a = src->a; dst->b = src->b; dst->c = src->c; }
/* equiv: {...src, d}: copy the source fields, then store the extra field */
static void NOINL v_object_spread_build(struct v_abcd *r, const struct v_abc *src, int32_t d) { r->a = src->a; r->b = src->b; r->c = src->c; r->d = d; }
/* equiv: fresh struct copy of a 3-field struct per iteration */
static double NOINL op_v_object_assign(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_abc src = {1, 2, 3}, *sp = &src;
  OPAQUE_P(sp);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    sp->b = i;
    v_arena_top = 0;
    struct v_abc *o = v_arena_alloc(sizeof *o);
    v_object_assign_copy(o, sp);
    s = (int32_t)((uint32_t)s + (uint32_t)o->a + (uint32_t)o->b + (uint32_t)o->c);
  }
  return s;
}
/* equiv: fresh 4-field struct built from the 3-field source plus the extra field */
static double NOINL op_v_object_spread(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_abc src = {1, 2, 3}, *sp = &src;
  OPAQUE_P(sp);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    sp->b = i;
    v_arena_top = 0;
    struct v_abcd *r = v_arena_alloc(sizeof *r);
    v_object_spread_build(r, sp, i);
    s = (int32_t)((uint32_t)s + (uint32_t)r->a + (uint32_t)r->b + (uint32_t)r->c + (uint32_t)r->d);
  }
  return s;
}

/* ---- v_string_keys_obj_build (object as a small string-keyed table) ---- */
struct v_kv { const char *key; int32_t val; int32_t used; };
static const char *const v_string_keys_obj_build_keys[4] = {"alpha", "beta", "gamma", "delta"};
/* equiv: FNV-1a over the key bytes */
static uint32_t NOINL v_string_keys_obj_build_hash(const char *s) {
  uint32_t h = 2166136261u;
  for (; *s; s++) { h ^= (uint8_t)*s; h *= 16777619u; }
  return h;
}
/* equiv: open-addressing insert into a preallocated 8-slot table (computed string key set) */
static void NOINL v_string_keys_obj_build_put(struct v_kv *t, const char *k, int32_t v) {
  uint32_t h = v_string_keys_obj_build_hash(k) & 7u;
  while (t[h].used && strcmp(t[h].key, k) != 0) h = (h + 1u) & 7u;
  t[h].key = k; t[h].val = v; t[h].used = 1;
}
/* equiv: open-addressing lookup by string key */
static int32_t NOINL v_string_keys_obj_build_get(const struct v_kv *t, const char *k) {
  uint32_t h = v_string_keys_obj_build_hash(k) & 7u;
  while (t[h].used && strcmp(t[h].key, k) != 0) h = (h + 1u) & 7u;
  return t[h].val;
}
/* equiv: fresh 8-slot string-keyed open-addressing table per iteration: 4 inserts, 1 lookup */
static double NOINL op_v_string_keys_obj_build(long n) {
  int32_t N = (int32_t)n, s = 0;
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct v_kv t[8];
    memset(t, 0, sizeof t); /* equiv: a fresh empty object */
    for (int32_t j = 0; j < 4; j++) v_string_keys_obj_build_put(t, v_string_keys_obj_build_keys[j], i + j);
    s = (int32_t)((uint32_t)s + (uint32_t)v_string_keys_obj_build_get(t, v_string_keys_obj_build_keys[i & 3]));
  }
  return s;
}

/* ---- v_bigint_add (BigInt.asIntN(64, a + k + i)) ---- */
/* equiv: int64_t add with two's-complement wrap (BigInt.asIntN(64)), low 32 bits returned */
static double NOINL op_v_bigint_add(long n) {
  int32_t N = (int32_t)n;
  int64_t a = 1;
  const uint64_t K = 0x9e3779b97f4a7c15ull;
  OPAQUE_P(a);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (int64_t)((uint64_t)a + K + (uint64_t)(int64_t)i); /* int64 wrap, equal to asIntN(64) */
  }
  return (double)((uint64_t)a & 0xffffffffull); /* asUintN(32): low 32 bits */
}

/* ---- v_int53_arith (exact integers beyond int32, a < 2^31 so a*31+i < 2^53) ---- */
/* equiv: int64_t multiply-add with modulo reduction, exact below 2^53 */
static double NOINL op_v_int53_arith(long n) {
  int32_t N = (int32_t)n;
  int64_t a = 1;
  OPAQUE_P(a);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    a = (a * 31 + i) % 2147483647;
  }
  return (double)a;
}

/* ---- v_dataview_get (little-endian uint32 reads from a byte buffer) ---- */
/* equiv: DataView getUint32(off, true): assemble the four bytes little-endian */
static uint32_t NOINL v_dataview_get_u32le(const uint8_t *p) {
  return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
/* equiv: byte buffer read with little-endian uint32 assembled from four bytes */
static double NOINL op_v_dataview_get(long n) {
  int32_t N = (int32_t)n, s = 0;
  uint8_t buf[64];
  memset(buf, 0, sizeof buf);
  for (int32_t j = 0; j < 16; j++) {
    uint32_t v = (uint32_t)((uint64_t)j * 2654435761ull);
    buf[j * 4 + 0] = (uint8_t)v; buf[j * 4 + 1] = (uint8_t)(v >> 8); buf[j * 4 + 2] = (uint8_t)(v >> 16); buf[j * 4 + 3] = (uint8_t)(v >> 24);
  }
  ESCAPE(buf);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)((uint32_t)s + v_dataview_get_u32le(buf + (i & 15) * 4));
  }
  return s;
}

/* ---- v_unipoker_rank (bit-manipulation hand rank with a popcount lookup table) ---- */
/* equiv: popcount lookup table plus bit masks for a 5-card hand rank */
static double NOINL op_v_unipoker_rank(long n) {
  int32_t N = (int32_t)n, s = 0;
  uint8_t pop[256];
  for (int32_t k = 0; k < 256; k++) { int32_t c = 0; for (int32_t x = k; x != 0; x &= x - 1) c++; pop[k] = (uint8_t)c; }
  ESCAPE(pop);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    int32_t ranks = 0, suits = 0;
    for (int32_t k = 0; k < 5; k++) {
      int32_t card = (i * 7 + k * k * 11 + (i >> 3) * k) % 52;
      ranks |= 1 << (card >> 2);
      suits |= 1 << (card & 3);
    }
    int32_t distinct = pop[ranks & 255] + pop[(ranks >> 8) & 255];
    int32_t straight = (ranks & (ranks >> 1) & (ranks >> 2) & (ranks >> 3) & (ranks >> 4)) != 0 ? 1 : 0;
    int32_t flush = (suits & (suits - 1)) == 0 ? 1 : 0;
    s = (int32_t)((uint32_t)s + (uint32_t)((flush * 2 + straight) * 8 + distinct));
  }
  return s;
}

/* ---- v_basic_interp (switch dispatch over 4 opcodes, n dispatches) ---- */
static const uint8_t v_basic_interp_code[16] = {0, 2, 1, 3, 3, 0, 1, 2, 2, 1, 0, 3, 1, 1, 2, 0};
/* equiv: switch-dispatch bytecode loop over 4 opcodes, one dispatch per iteration */
static double NOINL op_v_basic_interp(long n) {
  int32_t N = (int32_t)n, acc = 1;
  uint32_t pc = 0;
  const uint8_t *code = v_basic_interp_code;
  OPAQUE_P(code);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    switch (code[pc]) {
      case 0: acc = (int32_t)((uint32_t)acc + 1u); break;
      case 1: acc = acc ^ 0x5a; break;
      case 2: acc = (int32_t)((uint32_t)acc * 3u); break;
      case 3: acc = acc >> 1; break;
    }
    pc = (pc + 1u) & 15u;
  }
  return acc;
}

/* ---- v_air_regalloc_like (array of small structs: read two fields, write a third) ---- */
struct v_air { int32_t a, b, c; };
/* equiv: array of structs: element pointer, read two fields, write a third */
static double NOINL op_v_air_regalloc_like(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_air objs[64];
  for (int32_t j = 0; j < 64; j++) { objs[j].a = j; objs[j].b = j * 2; objs[j].c = 0; }
  ESCAPE(objs);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    struct v_air *o = &objs[i & 63]; /* equiv: array element address, no allocation */
    o->c = (int32_t)((uint32_t)o->a + (uint32_t)o->b);
    s = (int32_t)((uint32_t)s + (uint32_t)o->c);
  }
  return s;
}

/* ---- v_ml_dot_sigmoid (8-element dot product, then logistic activation) ---- */
static const double v_ml_dot_sigmoid_w[8] = {0.5, -0.25, 0.125, 1, -2, 0.75, 0.0625, -0.5};
/* equiv: 8-element dot product of a fixed weight vector and an input vector */
static double NOINL v_ml_dot_sigmoid_dot8(const double *w, int32_t i) {
  double d = 0;
  for (int32_t j = 0; j < 8; j++) d += w[j] * (double)(((i + j) & 15) - 8);
  return d;
}
/* equiv: logistic activation through libm exp */
static double NOINL v_ml_dot_sigmoid_act(double z) { return 1.0 / (1.0 + exp(-z)); }
/* equiv: 8-term dot product, then logistic activation through libm exp */
static double NOINL op_v_ml_dot_sigmoid(long n) {
  int32_t N = (int32_t)n;
  double s = 0;
  const double *w = v_ml_dot_sigmoid_w;
  OPAQUE_P(w);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    double z = v_ml_dot_sigmoid_dot8(w, i) * 0.125;
    s += floor(v_ml_dot_sigmoid_act(z) * 1048576.0); /* quantized to 2^-20 so libm vs V8 exp ulp differences cannot change the sum */
  }
  return s;
}

/* ---- v_weakmap_get (16 object keys, looked up by identity) ---- */
struct v_wm_key { int32_t id; };
struct v_wm_slot { const struct v_wm_key *key; int32_t val; };
/* equiv: hash of the object address (multiplicative, top 5 bits) into a 32-slot table */
static uint32_t NOINL v_weakmap_get_slot(const struct v_wm_key *k) { return (uint32_t)((((uint64_t)(uintptr_t)k >> 4) * 0x9e3779b97f4a7c15ull) >> 59); }
/* equiv: WeakMap.set, open addressing keyed by pointer identity */
static void NOINL v_weakmap_get_set(struct v_wm_slot *t, const struct v_wm_key *k, int32_t v) {
  uint32_t h = v_weakmap_get_slot(k);
  while (t[h].key != NULL && t[h].key != k) h = (h + 1u) & 31u;
  t[h].key = k; t[h].val = v;
}
/* equiv: WeakMap.get; returns 0 where JS would return undefined (never hit here: all keys are set) */
static int32_t NOINL v_weakmap_get_get(const struct v_wm_slot *t, const struct v_wm_key *k) {
  uint32_t h = v_weakmap_get_slot(k);
  while (t[h].key != NULL && t[h].key != k) h = (h + 1u) & 31u;
  return t[h].key == NULL ? 0 : t[h].val;
}
/* equiv: pointer-keyed open-addressing hash table, get by object identity */
static double NOINL op_v_weakmap_get(long n) {
  int32_t N = (int32_t)n, s = 0;
  struct v_wm_key keys[16];
  struct v_wm_slot t[32];
  memset(t, 0, sizeof t);
  for (int32_t j = 0; j < 16; j++) { keys[j].id = j; v_weakmap_get_set(t, &keys[j], j * 3); }
  ESCAPE(keys); ESCAPE(t);
  for (int32_t i = 0; i < N; i++) {
    BODY_BARRIER();
    s = (int32_t)((uint32_t)s + (uint32_t)v_weakmap_get_get(t, &keys[i & 15]));
  }
  return s;
}

static const struct { const char *name; opfn fn; } TABLE[] = {
  {"field_read", op_field_read}, {"field_name", op_field_read}, {"field_length", op_field_read}, {"field_store", op_field_store}, {"call_direct", op_call_direct},
  {"call_closure", op_call_closure}, {"call_rest", op_call_rest}, {"call_spread", op_call_spread}, {"call_rest_forward", op_call_rest_forward}, {"call_method", op_call_method},
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
  {"str_includes", op_str_includes}, {"str_slice", op_str_slice}, {"arr_isarray", op_arr_isarray}, {"arr_slice", op_arr_slice},
  {"str_index", op_str_index}, {"str_concat", op_str_concat},
  {"str_concat_acc", op_str_concat_acc}, {"str_compare", op_str_compare},
  {"arr_load_int", op_arr_load_int}, {"arr_store_int", op_arr_store_int},
  {"arr_load_dbl", op_arr_load_dbl}, {"arr_store_dbl", op_arr_store_dbl},
  {"arr_load_obj", op_arr_load_obj}, {"arr_store_obj", op_arr_store_obj},
  {"arr_length", op_arr_length}, {"arr_push", op_arr_push},
  {"typed_load", op_typed_load}, {"typed_store_f64", op_typed_store_f64},
  {"rx_lit", op_rx_lit}, {"rx_digits", op_rx_digits}, {"rx_word2", op_rx_word2},
  {"rx_email", op_rx_email}, {"rx_dotstar", op_rx_dotstar}, {"rx_ahead", op_rx_ahead},
  {"typed_load", op_typed_load}, {"typed_store_f64", op_typed_store_f64}, {"typed_store_u8c", op_typed_store_u8c},
  {"k_nbody_pair", op_k_nbody_pair}, {"k_struct_sum", op_k_struct_sum}, {"k_dispatch", op_k_dispatch},
  {"k_dist2", op_k_dist2}, {"k_field_acc", op_k_field_acc}, {"k_list_walk", op_k_list_walk}, {"k_vec_local", op_k_vec_local},
  {"obj_spread", op_obj_spread}, {"alloc_literal", op_alloc_literal}, {"alloc_literal1", op_alloc_literal1}, {"alloc_ctor", op_alloc_ctor}, {"alloc_class", op_alloc_class},
  {"k_json", op_k_json}, {"k_regexp", op_k_regexp}, {"k_str_build", op_k_str_build},
  {"date_now", op_date_now}, {"arr_includes", op_arr_includes}, {"arr_indexof", op_arr_indexof},
  {"bi_dv_get_f64", op_bi_dv_get_f64}, {"bi_dv_set_f64", op_bi_dv_set_f64}, {"bi_dv_get_u32", op_bi_dv_get_u32},
  {"bi_arr_slice", op_bi_arr_slice}, {"bi_arr_concat", op_bi_arr_concat}, {"bi_tmpl", op_bi_tmpl}, {"bi_map_get", op_bi_map_get}, {"bi_set_has", op_bi_set_has},
  {"bi_fill", op_bi_fill}, {"bi_join", op_bi_join}, {"bi_from", op_bi_from}, {"bi_upper", op_bi_upper}, {"bi_trim", op_bi_trim}, {"bi_pad", op_bi_pad}, {"bi_sindex", op_bi_sindex}, {"bi_sslice", op_bi_sslice}, {"bi_split", op_bi_split}, {"bi_replace", op_bi_replace}, {"bi_sort_int", op_bi_sort_int}, {"bi_sort_cmp", op_bi_sort_cmp}, {"bi_map", op_bi_map}, {"bi_parse", op_bi_parse}, {"bi_keys", op_bi_keys}, {"bi_assign", op_bi_assign},
  {"k01_sum_or", op_k01_sum_or},
  {"k02_fib", op_k02_fib},
  {"k03_call_add", op_k03_call_add},
  {"k04_dot", op_k04_dot},
  {"k05_field_add", op_k05_field_add},
  {"k06_list", op_k06_list},
  {"k07_area", op_k07_area},
  {"k08_escape", op_k08_escape},
  {"k09_reduce", op_k09_reduce},
  {"k10_f64_sum", op_k10_f64_sum},
  {"k11_saxpy", op_k11_saxpy},
  {"k12_copy", op_k12_copy},
  {"k13_matmul", op_k13_matmul},
  {"k14_bsearch", op_k14_bsearch},
  {"k15_isort", op_k15_isort},
  {"k16_sieve", op_k16_sieve},
  {"k17_hash", op_k17_hash},
  {"k18_math", op_k18_math},
  {"k19_gcd", op_k19_gcd},
  {"k20_switch", op_k20_switch},
  {"c_while", op_c_while},
  {"c_do_while", op_c_do_while},
  {"c_for_down", op_c_for_down},
  {"c_loop_step2", op_c_loop_step2},
  {"c_nested_loop", op_c_nested_loop},
  {"c_loop_break", op_c_loop_break},
  {"c_loop_continue", op_c_loop_continue},
  {"c_loop_if_else", op_c_loop_if_else},
  {"c_loop_if_chain", op_c_loop_if_chain},
  {"c_ternary", op_c_ternary},
  {"c_select_minmax", op_c_select_minmax},
  {"c_switch_dense", op_c_switch_dense},
  {"c_switch_sparse", op_c_switch_sparse},
  {"c_early_return", op_c_early_return},
  {"c_bool_flag", op_c_bool_flag},
  {"c_logical_and_or", op_c_logical_and_or},
  {"c_locals_rotate", op_c_locals_rotate},
  {"c_int_expr", op_c_int_expr},
  {"c_arr_sum_int", op_c_arr_sum_int}, {"c_arr_sum_dbl", op_c_arr_sum_dbl}, {"c_arr_store_seq", op_c_arr_store_seq},
  {"c_arr_rmw", op_c_arr_rmw}, {"c_arr_stencil", op_c_arr_stencil}, {"c_arr_swap", op_c_arr_swap},
  {"c_arr_indirect", op_c_arr_indirect}, {"c_arr_histogram", op_c_arr_histogram}, {"c_arr_2d", op_c_arr_2d},
  {"c_arr_len_loop", op_c_arr_len_loop}, {"c_i32_sum", op_c_i32_sum}, {"c_i32_store", op_c_i32_store},
  {"c_u8_rmw", op_c_u8_rmw}, {"c_u16_load", op_c_u16_load}, {"c_f32_mad", op_c_f32_mad},
  {"c_f64_dot", op_c_f64_dot}, {"c_i32_indirect", op_c_i32_indirect},
  {"c_struct_get2", op_c_struct_get2},
  {"c_struct_set", op_c_struct_set},
  {"c_struct_rmw", op_c_struct_rmw},
  {"c_struct_array", op_c_struct_array},
  {"c_struct_nested", op_c_struct_nested},
  {"c_struct_dbl", op_c_struct_dbl},
  {"c_class_method", op_c_class_method},
  {"c_class_ctor_fields", op_c_class_ctor_fields},
  {"c_call_leaf", op_c_call_leaf},
  {"c_call_4args", op_c_call_4args},
  {"c_call_2deep", op_c_call_2deep},
  {"c_call_void", op_c_call_void},
  {"c_call_recursive_depth4", op_c_call_recursive_depth4},
  {"c_call_dbl", op_c_call_dbl},
  {"c_closure_read", op_c_closure_read},
  {"c_method_poly2", op_c_method_poly2},
  {"c_return_early", op_c_return_early},
  {"c_dbl_expr", op_c_dbl_expr},
  {"c_dbl_cmp_branch", op_c_dbl_cmp_branch},
  {"c_dbl_to_int", op_c_dbl_to_int},
  {"c_int_to_dbl", op_c_int_to_dbl},
  {"c_int_div_const", op_c_int_div_const},
  {"c_int_mod_pow2", op_c_int_mod_pow2},
  {"c_popcount", op_c_popcount},
  {"c_bit_test", op_c_bit_test},
  {"c_rotate", op_c_rotate},
  {"c_hash_mix", op_c_hash_mix},
  {"c_abs_int", op_c_abs_int},
  {"c_clamp", op_c_clamp},
  {"c_str_char_scan", op_c_str_char_scan},
  {"c_str_hash", op_c_str_hash},
  {"c_str_index_eq", op_c_str_index_eq},
  {"c_str_len_loop", op_c_str_len_loop},
  {"c_counter_dec", op_c_counter_dec},
  {"v_proto_method", op_v_proto_method},
  {"v_proto_chain3_field", op_v_proto_chain3_field},
  {"v_poly_method3", op_v_poly_method3},
  {"v_poly_method8", op_v_poly_method8},
  {"v_super_call", op_v_super_call},
  {"v_getter", op_v_getter},
  {"v_setter", op_v_setter},
  {"v_field_poly4", op_v_field_poly4},
  {"v_instanceof", op_v_instanceof},
  {"v_new_point", op_v_new_point},
  {"v_obj_literal", op_v_obj_literal},
  {"v_linked_list_walk", op_v_linked_list_walk},
  {"v_closure_call", op_v_closure_call},
  {"v_closure_counter", op_v_closure_counter},
  {"v_bind_call", op_v_bind_call},
  {"v_fn_call", op_v_fn_call},
  {"v_fn_apply", op_v_fn_apply},
  {"v_arguments_length", op_v_arguments_length},
  {"v_richards_task", op_v_richards_task},
  {"v_deltablue_constraint", op_v_deltablue_constraint},
  {"v_array_push_pop", op_v_array_push_pop},
  {"v_array_push_grow", op_v_array_push_grow},
  {"v_array_2d", op_v_array_2d},
  {"v_navier_lin_solve", op_v_navier_lin_solve},
  {"v_f64_axpy", op_v_f64_axpy},
  {"v_u8_bytes", op_v_u8_bytes},
  {"v_crypto_am3", op_v_crypto_am3},
  {"v_sha_round", op_v_sha_round},
  {"v_nbody_step", op_v_nbody_step},
  {"v_float_mm", op_v_float_mm},
  {"v_int_to_double", op_v_int_to_double},
  {"v_math_sin_cos", op_v_math_sin_cos},
  {"v_array_foreach", op_v_array_foreach},
  {"v_array_map", op_v_array_map},
  {"v_array_reduce", op_v_array_reduce},
  {"v_array_indexof", op_v_array_indexof},
  {"v_array_holey", op_v_array_holey},
  {"v_array_of_objects_field", op_v_array_of_objects_field},
  {"v_typed_from_array_sort", op_v_typed_from_array_sort},
  {"v_gaussian_blur", op_v_gaussian_blur},
  {"v_str_build", op_v_str_build},
  {"v_str_charcode_scan", op_v_str_charcode_scan},
  {"v_str_fromcharcode", op_v_str_fromcharcode},
  {"v_str_indexof", op_v_str_indexof},
  {"v_str_slice", op_v_str_slice},
  {"v_str_split_join", op_v_str_split_join},
  {"v_str_template", op_v_str_template},
  {"v_str_compare_lt", op_v_str_compare_lt},
  {"v_str_tolower", op_v_str_tolower},
  {"v_str_eq_dynamic", op_v_str_eq_dynamic},
  {"v_num_tostring", op_v_num_tostring},
  {"v_parse_int", op_v_parse_int},
  {"v_parse_float", op_v_parse_float},
  {"v_regexp_test", op_v_regexp_test},
  {"v_regexp_exec_groups", op_v_regexp_exec_groups},
  {"v_str_replace_regexp", op_v_str_replace_regexp},
  {"v_json_stringify", op_v_json_stringify},
  {"v_json_parse", op_v_json_parse},
  {"v_str_key_dict", op_v_str_key_dict},
  {"v_str_concat3", op_v_str_concat3},
  {"v_map_get_int", op_v_map_get_int}, {"v_map_set_delete", op_v_map_set_delete}, {"v_map_get_str", op_v_map_get_str}, {"v_set_has", op_v_set_has},
  {"v_map_iterate", op_v_map_iterate}, {"v_obj_keys_loop", op_v_obj_keys_loop}, {"v_for_in", op_v_for_in}, {"v_for_of_array", op_v_for_of_array},
  {"v_generator_next", op_v_generator_next}, {"v_try_nothrow", op_v_try_nothrow}, {"v_throw_catch", op_v_throw_catch}, {"v_spread_call", op_v_spread_call},
  {"v_rest_args", op_v_rest_args}, {"v_destructure_array", op_v_destructure_array}, {"v_destructure_obj", op_v_destructure_obj}, {"v_optional_chain", op_v_optional_chain},
  {"v_nullish", op_v_nullish}, {"v_splay_insert_find", op_v_splay_insert_find}, {"v_astar_heap", op_v_astar_heap}, {"v_switch_string", op_v_switch_string},
  {"v_alloc_small", op_v_alloc_small}, {"v_alloc_array4", op_v_alloc_array4}, {"v_alloc_closure", op_v_alloc_closure},
  {"v_alloc_retained", op_v_alloc_retained}, {"v_cons_list", op_v_cons_list}, {"v_vec3_ops", op_v_vec3_ops},
  {"v_box2d_vec2", op_v_box2d_vec2}, {"v_class_hierarchy_ctor", op_v_class_hierarchy_ctor}, {"v_array_literal_objects", op_v_array_literal_objects},
  {"v_object_assign", op_v_object_assign}, {"v_object_spread", op_v_object_spread}, {"v_string_keys_obj_build", op_v_string_keys_obj_build},
  {"v_bigint_add", op_v_bigint_add}, {"v_int53_arith", op_v_int53_arith}, {"v_dataview_get", op_v_dataview_get},
  {"v_unipoker_rank", op_v_unipoker_rank}, {"v_basic_interp", op_v_basic_interp}, {"v_air_regalloc_like", op_v_air_regalloc_like},
  {"v_ml_dot_sigmoid", op_v_ml_dot_sigmoid}, {"v_weakmap_get", op_v_weakmap_get},
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
