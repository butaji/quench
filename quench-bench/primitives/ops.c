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
