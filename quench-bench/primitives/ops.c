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
