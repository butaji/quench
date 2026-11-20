// jetstream-row: crypto
// jetstream-files: Octane/crypto.js
// iterations: 120
// jetstream-driver: deterministic=true
/* C analogue of JetStream 3's Octane/crypto.js (Tom Wu's JavaScript RSA on a BigInteger class):
 * the same 28-bit digit representation and am3 inner loop, the same Montgomery reduction,
 * CRT private-key path with the windowed modPow, the same PKCS#1 v1.5 padding drawn from an
 * RC4 SecureRandom, and the same 1024-bit key (e = 65537, CRT parts of 512 bits). Each
 * iteration builds the key, encrypts TEXT, then decrypts it and checks the round trip, as
 * crypto.js's runIteration does; a mismatch is a failure, as the JS throw is.
 * Differences: the RNG pool is filled with a fixed pattern instead of Math.random(); the
 * JS paths the RSA route never reaches (generate, radix conversion, Classic and Barrett
 * reduction, even moduli) are not ported. BigInteger objects come from a slot pool that is
 * reset at the start of each encrypt() or decrypt(), as a C program frees a call's temporaries
 * at once where the JS collector reclaims them. */
#define _POSIX_C_SOURCE 199309L
#pragma STDC FP_CONTRACT OFF
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef int64_t i64;

#define BI_DB 28
#define BI_DM ((i64)((1 << BI_DB) - 1))
#define BI_DV ((i64)1 << BI_DB)
#define BI_FP 52
#define BI_F1 (BI_FP - BI_DB)
#define BI_F2 (2 * BI_DB - BI_FP)
#define BI_FV 4503599627370496.0 /* 2^52 */
#define BI_MAXD 160
#define BI_SLOTS 4096
#define BI_RM "0123456789abcdefghijklmnopqrstuvwxyz"

typedef struct BI { int t, s; i64 a[BI_MAXD]; } BI;

static BI slots[BI_SLOTS];
static int slots_used;
static BI zero_obj, one_obj;
static BI *bi_zero = &zero_obj, *bi_one = &one_obj;

static BI *nbi(void) {
  if (slots_used >= BI_SLOTS) { fprintf(stderr, "bigint pool exhausted\n"); exit(1); }
  BI *b = &slots[slots_used++];
  memset(b, 0, sizeof *b);
  return b;
}

static BI *bi_newInt(i64 v) {
  BI *r = nbi();
  r->t = 1; r->s = v < 0 ? -1 : 0;
  if (v > 0) r->a[0] = v;
  else if (v < -1) r->a[0] = v + BI_DV;
  else r->t = 0;
  return r;
}

static int intAt(char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'z') return c - 'a' + 10;
  if (c >= 'A' && c <= 'Z') return c - 'A' + 10;
  return -1;
}

static void bi_clamp(BI *x) {
  i64 c = (i64)x->s & BI_DM;
  while (x->t > 0 && x->a[x->t - 1] == c) --x->t;
}

static void bi_copyTo(BI *x, BI *r) {
  for (int i = x->t - 1; i >= 0; --i) r->a[i] = x->a[i];
  r->t = x->t;
  r->s = x->s;
}

static int nbits(i64 x) {
  int r = 1;
  i64 t;
  if ((t = x >> 16) != 0) { x = t; r += 16; }
  if ((t = x >> 8) != 0) { x = t; r += 8; }
  if ((t = x >> 4) != 0) { x = t; r += 4; }
  if ((t = x >> 2) != 0) { x = t; r += 2; }
  if ((t = x >> 1) != 0) { x = t; r += 1; }
  return r;
}

static int bi_bitLength(BI *x) {
  if (x->t <= 0) return 0;
  return BI_DB * (x->t - 1) + nbits(x->a[x->t - 1] ^ ((i64)x->s & BI_DM));
}

static void bi_dlShiftTo(BI *x, int n, BI *r) {
  int i;
  for (i = x->t - 1; i >= 0; --i) r->a[i + n] = x->a[i];
  for (i = n - 1; i >= 0; --i) r->a[i] = 0;
  r->t = x->t + n;
  r->s = x->s;
}

static void bi_drShiftTo(BI *x, int n, BI *r) {
  for (int i = n; i < x->t; ++i) r->a[i - n] = x->a[i];
  r->t = x->t - n > 0 ? x->t - n : 0;
  r->s = x->s;
}

static void bi_lShiftTo(BI *x, int n, BI *r) {
  int bs = n % BI_DB;
  int cbs = BI_DB - bs;
  i64 bm = ((i64)1 << cbs) - 1;
  int ds = n / BI_DB;
  i64 c = ((i64)x->s * ((i64)1 << bs)) & BI_DM;
  int i;
  for (i = x->t - 1; i >= 0; --i) {
    r->a[i + ds + 1] = (x->a[i] >> cbs) | c;
    c = (x->a[i] & bm) << bs;
  }
  for (i = ds - 1; i >= 0; --i) r->a[i] = 0;
  r->a[ds] = c;
  r->t = x->t + ds + 1;
  r->s = x->s;
  bi_clamp(r);
}

static void bi_rShiftTo(BI *x, int n, BI *r) {
  r->s = x->s;
  int ds = n / BI_DB;
  if (ds >= x->t) { r->t = 0; return; }
  int bs = n % BI_DB;
  int cbs = BI_DB - bs;
  i64 bm = ((i64)1 << bs) - 1;
  r->a[0] = x->a[ds] >> bs;
  for (int i = ds + 1; i < x->t; ++i) {
    r->a[i - ds - 1] |= (x->a[i] & bm) << cbs;
    r->a[i - ds] = x->a[i] >> bs;
  }
  if (bs > 0) r->a[x->t - ds - 1] |= ((i64)x->s & bm) << cbs;
  r->t = x->t - ds;
  bi_clamp(r);
}

static void bi_subTo(BI *x, BI *a, BI *r) {
  int i = 0, m = a->t < x->t ? a->t : x->t;
  i64 c = 0;
  while (i < m) {
    c += x->a[i] - a->a[i];
    r->a[i++] = c & BI_DM;
    c >>= BI_DB;
  }
  if (a->t < x->t) {
    c -= a->s;
    while (i < x->t) {
      c += x->a[i];
      r->a[i++] = c & BI_DM;
      c >>= BI_DB;
    }
    c += x->s;
  } else {
    c += x->s;
    while (i < a->t) {
      c -= a->a[i];
      r->a[i++] = c & BI_DM;
      c >>= BI_DB;
    }
    c -= a->s;
  }
  r->s = (c < 0) ? -1 : 0;
  if (c < -1) r->a[i++] = BI_DV + c;
  else if (c > 0) r->a[i++] = c;
  r->t = i;
  bi_clamp(r);
}

static void bi_addTo(BI *x, BI *a, BI *r) {
  int i = 0, m = a->t < x->t ? a->t : x->t;
  i64 c = 0;
  while (i < m) {
    c += x->a[i] + a->a[i];
    r->a[i++] = c & BI_DM;
    c >>= BI_DB;
  }
  if (a->t < x->t) {
    c += a->s;
    while (i < x->t) {
      c += x->a[i];
      r->a[i++] = c & BI_DM;
      c >>= BI_DB;
    }
    c += x->s;
  } else {
    c += x->s;
    while (i < a->t) {
      c += a->a[i];
      r->a[i++] = c & BI_DM;
      c >>= BI_DB;
    }
    c += a->s;
  }
  r->s = (c < 0) ? -1 : 0;
  if (c > 0) r->a[i++] = c;
  else if (c < -1) r->a[i++] = BI_DV + c;
  r->t = i;
  bi_clamp(r);
}

static BI *bi_add(BI *x, BI *a) { BI *r = nbi(); bi_addTo(x, a, r); return r; }
static BI *bi_subtract(BI *x, BI *a) { BI *r = nbi(); bi_subTo(x, a, r); return r; }

static BI *bi_negate(BI *x) { BI *r = nbi(); bi_subTo(bi_zero, x, r); return r; }
static BI *bi_abs(BI *x) { return x->s < 0 ? bi_negate(x) : x; }

/* am3: the 28-bit digit multiply-accumulate of crypto.js (dbits = 28, 14-bit halves). */
static i64 am3(BI *self, int i, i64 x, BI *w, int j, i64 c, int n) {
  i64 xl = x & 0x3fff, xh = x >> 14;
  while (--n >= 0) {
    i64 l = self->a[i] & 0x3fff;
    i64 h = self->a[i] >> 14;
    i++;
    i64 m = xh * l + h * xl;
    l = xl * l + ((m & 0x3fff) << 14) + w->a[j] + c;
    c = (l >> 28) + (m >> 14) + xh * h;
    w->a[j++] = l & 0xfffffff;
  }
  return c;
}

static void bi_multiplyTo(BI *x0, BI *a, BI *r) {
  BI *x = bi_abs(x0), *y = bi_abs(a);
  int i = x->t;
  r->t = i + y->t;
  while (--i >= 0) r->a[i] = 0;
  for (i = 0; i < y->t; ++i) r->a[i + x->t] = am3(x, 0, y->a[i], r, i, 0, x->t);
  r->s = 0;
  bi_clamp(r);
  if (x0->s != a->s) bi_subTo(bi_zero, r, r);
}

static BI *bi_multiply(BI *x, BI *a) { BI *r = nbi(); bi_multiplyTo(x, a, r); return r; }

static void bi_squareTo(BI *x0, BI *r) {
  BI *x = bi_abs(x0);
  int i = r->t = 2 * x->t;
  while (--i >= 0) r->a[i] = 0;
  for (i = 0; i < x->t - 1; ++i) {
    i64 c = am3(x, i, x->a[i], r, 2 * i, 0, 1);
    i64 carry = am3(x, i + 1, 2 * x->a[i], r, 2 * i + 1, c, x->t - i - 1);
    r->a[i + x->t] += carry;
    if (r->a[i + x->t] >= BI_DV) {
      r->a[i + x->t] -= BI_DV;
      r->a[i + x->t + 1] = 1;
    }
  }
  if (r->t > 0) {
    i64 carry = am3(x, i, x->a[i], r, 2 * i, 0, 1);
    r->a[r->t - 1] += carry;
  }
  r->s = 0;
  bi_clamp(r);
}

static int bi_compareTo(BI *x, BI *a) {
  i64 r = (i64)x->s - a->s;
  if (r != 0) return r < 0 ? -1 : 1;
  int i = x->t;
  r = i - a->t;
  if (r != 0) return r < 0 ? -1 : 1;
  while (--i >= 0) if ((r = x->a[i] - a->a[i]) != 0) return r < 0 ? -1 : 1;
  return 0;
}

static void bi_divRemTo(BI *self, BI *m, BI *q, BI *r) {
  BI *pm = bi_abs(m);
  if (pm->t <= 0) return;
  BI *pt = bi_abs(self);
  if (pt->t < pm->t) {
    if (q != NULL) { q->t = 0; q->s = 0; }
    if (r != NULL) bi_copyTo(self, r);
    return;
  }
  if (r == NULL) r = nbi();
  BI *y = nbi();
  int ts = self->s, ms = m->s;
  int nsh = BI_DB - nbits(pm->a[pm->t - 1]); /* normalize modulus */
  if (nsh > 0) { bi_lShiftTo(pm, nsh, y); bi_lShiftTo(pt, nsh, r); }
  else { bi_copyTo(pm, y); bi_copyTo(pt, r); }
  int ys = y->t;
  i64 y0 = y->a[ys - 1];
  if (y0 == 0) return;
  i64 yt = y0 * ((i64)1 << BI_F1) + ((ys > 1) ? (y->a[ys - 2] >> BI_F2) : 0);
  double d1 = BI_FV / (double)yt;
  double d2 = (double)((i64)1 << BI_F1) / (double)yt;
  i64 e = (i64)1 << BI_F2;
  int i = r->t, j = i - ys;
  BI *t = (q == NULL) ? nbi() : q;
  bi_dlShiftTo(y, j, t);
  if (bi_compareTo(r, t) >= 0) {
    r->a[r->t++] = 1;
    bi_subTo(r, t, r);
  }
  bi_dlShiftTo(bi_one, ys, t);
  bi_subTo(t, y, y); /* "negative" y so the sub can be replaced by am */
  while (y->t < ys) y->a[y->t++] = 0;
  while (--j >= 0) {
    i64 qd;
    --i;
    if (r->a[i] == y0) qd = BI_DM;
    else qd = (i64)floor((double)r->a[i] * d1 + (double)(r->a[i - 1] + e) * d2);
    i64 carry = am3(y, 0, qd, r, j, 0, ys);
    r->a[i] += carry;
    if (r->a[i] < qd) { /* try it out */
      bi_dlShiftTo(y, j, t);
      bi_subTo(r, t, r);
      while (r->a[i] < --qd) bi_subTo(r, t, r);
    }
  }
  if (q != NULL) {
    bi_drShiftTo(r, ys, q);
    if (ts != ms) bi_subTo(bi_zero, q, q);
  }
  r->t = ys;
  bi_clamp(r);
  if (nsh > 0) bi_rShiftTo(r, nsh, r);
  if (ts < 0) bi_subTo(bi_zero, r, r);
}

static BI *bi_mod(BI *x, BI *a) {
  BI *r = nbi();
  bi_divRemTo(bi_abs(x), a, NULL, r);
  if (x->s < 0 && bi_compareTo(r, bi_zero) > 0) bi_subTo(a, r, r);
  return r;
}

/* Montgomery reduction context (bnpInvDigit and the Montgomery class). */
typedef struct { BI *m; i64 mp, mpl, mph, um; int mt2; } Mont;

static i64 bi_invDigit(BI *x0) {
  if (x0->t < 1) return 0;
  i64 x = x0->a[0];
  if ((x & 1) == 0) return 0;
  i64 y = x & 3;
  y = (y * (2 - (x & 0xf) * y)) & 0xf;
  y = (y * (2 - (x & 0xff) * y)) & 0xff;
  y = (y * (2 - (((x & 0xffff) * y) & 0xffff))) & 0xffff;
  y = (y * (2 - x * y % BI_DV)) % BI_DV;
  return (y > 0) ? BI_DV - y : -y;
}

static void mont_init(Mont *z, BI *m) {
  z->m = m;
  z->mp = bi_invDigit(m);
  z->mpl = z->mp & 0x7fff;
  z->mph = z->mp >> 15;
  z->um = ((i64)1 << (BI_DB - 15)) - 1;
  z->mt2 = 2 * m->t;
}

static void mont_reduce(Mont *z, BI *x) {
  BI *m = z->m;
  while (x->t <= z->mt2) x->a[x->t++] = 0;
  for (int i = 0; i < m->t; ++i) {
    i64 j = x->a[i] & 0x7fff;
    i64 u0 = (j * z->mpl + (((j * z->mph + (x->a[i] >> 15) * z->mpl) & z->um) << 15)) & BI_DM;
    j = i + m->t;
    i64 carry = am3(m, 0, u0, x, i, 0, m->t);
    x->a[j] += carry;
    while (x->a[j] >= BI_DV) {
      x->a[j] -= BI_DV;
      x->a[++j]++;
    }
  }
  bi_clamp(x);
  bi_drShiftTo(x, m->t, x);
  if (bi_compareTo(x, m) >= 0) bi_subTo(x, m, x);
}

static BI *mont_convert(Mont *z, BI *x) {
  BI *r = nbi();
  bi_dlShiftTo(bi_abs(x), z->m->t, r);
  bi_divRemTo(r, z->m, NULL, r);
  if (x->s < 0 && bi_compareTo(r, bi_zero) > 0) bi_subTo(z->m, r, r);
  return r;
}

static BI *mont_revert(Mont *z, BI *x) {
  BI *r = nbi();
  bi_copyTo(x, r);
  mont_reduce(z, r);
  return r;
}

static void mont_sqrTo(Mont *z, BI *x, BI *r) { bi_squareTo(x, r); mont_reduce(z, r); }
static void mont_mulTo(Mont *z, BI *x, BI *y, BI *r) { bi_multiplyTo(x, y, r); mont_reduce(z, r); }

/* bnpExp with a Montgomery context: base^e mod m for an int exponent. */
static BI *bi_exp(BI *base, i64 e, Mont *z) {
  if (e > 0xffffffffLL || e < 1) return bi_one;
  BI *r = nbi(), *r2 = nbi(), *g = mont_convert(z, base), *t;
  int i = nbits(e) - 1;
  bi_copyTo(g, r);
  while (--i >= 0) {
    mont_sqrTo(z, r, r2);
    if ((e & ((i64)1 << i)) > 0) mont_mulTo(z, r2, g, r);
    else { t = r; r = r2; r2 = t; }
  }
  return mont_revert(z, r);
}

/* bnModPowInt for odd m and e >= 256, the only case RSA reaches (e = 65537, n odd). */
static BI *bi_modPowInt(BI *base, i64 e, BI *m) {
  Mont z;
  mont_init(&z, m);
  return bi_exp(base, e, &z);
}

/* bnModPow with a sliding window over Montgomery form (odd modulus). */
static BI *bi_modPow(BI *base, BI *e, BI *m) {
  int i = bi_bitLength(e), k;
  BI *r = bi_newInt(1);
  if (i <= 0) return r;
  if (i < 18) k = 1;
  else if (i < 48) k = 3;
  else if (i < 144) k = 4;
  else if (i < 768) k = 5;
  else k = 6;
  Mont z;
  mont_init(&z, m);
  int k1 = k - 1, km = (1 << k) - 1, n = 3;
  BI *g[64];
  g[1] = mont_convert(&z, base);
  if (k > 1) {
    BI *g2 = nbi();
    mont_sqrTo(&z, g[1], g2);
    while (n <= km) {
      g[n] = nbi();
      mont_mulTo(&z, g2, g[n - 2], g[n]);
      n += 2;
    }
  }
  int j = e->t - 1, w, is1 = 1;
  BI *r2 = nbi(), *t;
  i = nbits(e->a[j]) - 1;
  while (j >= 0) {
    if (i >= k1) {
      w = (int)((e->a[j] >> (i - k1)) & km);
    } else {
      w = (int)((e->a[j] & (((i64)1 << (i + 1)) - 1)) << (k1 - i));
      if (j > 0) w |= (int)(e->a[j - 1] >> (BI_DB + i - k1));
    }
    n = k;
    while ((w & 1) == 0) { w >>= 1; --n; }
    if ((i -= n) < 0) { i += BI_DB; --j; }
    if (is1) { /* ret == 1: no squaring or multiplying needed */
      bi_copyTo(g[w], r);
      is1 = 0;
    } else {
      while (n > 1) { mont_sqrTo(&z, r, r2); mont_sqrTo(&z, r2, r); n -= 2; }
      if (n > 0) mont_sqrTo(&z, r, r2);
      else { t = r; r = r2; r2 = t; }
      mont_mulTo(&z, r2, g[w], r);
    }
    while (j >= 0 && (e->a[j] & ((i64)1 << i)) == 0) {
      mont_sqrTo(&z, r, r2);
      t = r; r = r2; r2 = t;
      if (--i < 0) { i = BI_DB - 1; --j; }
    }
  }
  return mont_revert(&z, r);
}

/* BigInteger.toString(16) for non-negative values (RSA never produces negatives here). */
static void bi_to_hex(BI *x, char *out) {
  const int k = 4;
  const i64 km = (1 << k) - 1;
  i64 d = 0;
  int m = 0, n = 0, i = x->t;
  int p = BI_DB - (i * BI_DB) % k;
  if (i-- > 0) {
    if (p < BI_DB && (d = x->a[i] >> p) > 0) { m = 1; out[n++] = BI_RM[d]; }
    while (i >= 0) {
      if (p < k) {
        d = (x->a[i] & (((i64)1 << p) - 1)) << (k - p);
        --i;
        p += BI_DB - k;
        d |= x->a[i] >> p;
      } else {
        p -= k;
        d = (x->a[i] >> p) & km;
        if (p <= 0) { p += BI_DB; --i; }
      }
      if (d > 0) m = 1;
      if (m) out[n++] = BI_RM[d];
    }
  }
  if (!m) { out[0] = '0'; n = 1; }
  out[n] = '\0';
}

/* new BigInteger(s, 16): fromString with 4-bit digits. */
static BI *bi_parse_hex(const char *s) {
  BI *x = nbi();
  const int k = 4;
  x->t = 0; x->s = 0;
  int i = (int)strlen(s), mi = 0, sh = 0;
  while (--i >= 0) {
    int xv = intAt(s[i]);
    if (xv < 0) {
      if (s[i] == '-') mi = 1;
      continue;
    }
    mi = 0;
    if (sh == 0) x->a[x->t++] = xv;
    else if (sh + k > BI_DB) {
      x->a[x->t - 1] |= ((i64)xv & (((i64)1 << (BI_DB - sh)) - 1)) << sh;
      x->a[x->t++] = (i64)xv >> (BI_DB - sh);
    } else {
      x->a[x->t - 1] |= (i64)xv << sh;
    }
    sh += k;
    if (sh >= BI_DB) sh -= BI_DB;
  }
  bi_clamp(x);
  if (mi) bi_subTo(bi_zero, x, x);
  return x;
}

/* new BigInteger(bytes): fromString with 8-bit digits (the pkcs1pad2 path). */
static BI *bi_from_bytes(const unsigned char *s, int len) {
  BI *x = nbi();
  const int k = 8;
  x->t = 0; x->s = 0;
  int i = len, sh = 0;
  while (--i >= 0) {
    i64 v = s[i] & 0xff;
    if (sh == 0) x->a[x->t++] = v;
    else if (sh + k > BI_DB) {
      x->a[x->t - 1] |= (v & (((i64)1 << (BI_DB - sh)) - 1)) << sh;
      x->a[x->t++] = v >> (BI_DB - sh);
    } else {
      x->a[x->t - 1] |= v << sh;
    }
    sh += k;
    if (sh >= BI_DB) sh -= BI_DB;
  }
  if (len > 0 && (s[0] & 0x80) != 0) {
    x->s = -1;
    if (sh > 0) x->a[x->t - 1] |= (((i64)1 << (BI_DB - sh)) - 1) << sh;
  }
  bi_clamp(x);
  return x;
}

/* toByteArray(): two's complement bytes, stored as signed ints in out. Returns the length. */
static int bi_toByteArray(BI *x, int *out) {
  int i = x->t, k = 0;
  i64 s = x->s;
  i64 d = 0;
  out[0] = (int)s;
  int p = BI_DB - (i * BI_DB) % 8;
  if (i-- > 0) {
    i64 top = 0;
    if (p < BI_DB && (top = x->a[i] >> p) != (s & BI_DM) >> p)
      out[k++] = (int)(top | (s * ((i64)1 << (BI_DB - p))));
    while (i >= 0) {
      if (p < 8) {
        d = (x->a[i] & (((i64)1 << p) - 1)) << (8 - p);
        --i;
        p += BI_DB - 8;
        d |= x->a[i] >> p;
      } else {
        p -= 8;
        d = (x->a[i] >> p) & 0xff;
        if (p <= 0) { p += BI_DB; --i; }
      }
      if ((d & 0x80) != 0) d |= -256;
      if (k == 0 && ((s & 0x80) != (d & 0x80))) ++k;
      if (k > 0 || d != s) out[k++] = (int)d;
    }
  }
  return k > 0 ? k : 1;
}

/* RC4-based SecureRandom, as in crypto.js. */
static unsigned char rng_pool[256];
static int rng_pptr;
static int rng_ready;
static unsigned char arc_S[256];
static int arc_i, arc_j;

static void rng_seed_int(i64 x) {
  rng_pool[rng_pptr++] ^= (unsigned char)(x & 255);
  rng_pool[rng_pptr++] ^= (unsigned char)((x >> 8) & 255);
  rng_pool[rng_pptr++] ^= (unsigned char)((x >> 16) & 255);
  rng_pool[rng_pptr++] ^= (unsigned char)((x >> 24) & 255);
  if (rng_pptr >= 256) rng_pptr -= 256;
}

static void rng_seed_time(void) { rng_seed_int(1122926989487LL); }

static void rng_init_pool(void) {
  /* crypto.js fills the pool from Math.random(); a fixed pattern keeps runs reproducible. */
  uint32_t seed = 0x2545F491u;
  for (int i = 0; i < 256; ++i) {
    seed = seed * 1103515245u + 12345u;
    rng_pool[i] = (unsigned char)(seed >> 16);
  }
  rng_pptr = 0;
  rng_seed_time();
}

static void arc4_init(const unsigned char *key) {
  int i, j = 0;
  unsigned char t;
  for (i = 0; i < 256; ++i) arc_S[i] = (unsigned char)i;
  for (i = 0; i < 256; ++i) {
    j = (j + arc_S[i] + key[i % 256]) & 255;
    t = arc_S[i];
    arc_S[i] = arc_S[j];
    arc_S[j] = t;
  }
  arc_i = 0;
  arc_j = 0;
}

static int arc4_next(void) {
  unsigned char t;
  arc_i = (arc_i + 1) & 255;
  arc_j = (arc_j + arc_S[arc_i]) & 255;
  t = arc_S[arc_i];
  arc_S[arc_i] = arc_S[arc_j];
  arc_S[arc_j] = t;
  return arc_S[(t + arc_S[arc_i]) & 255];
}

static int rng_get_byte(void) {
  if (!rng_ready) {
    rng_seed_time();
    arc4_init(rng_pool);
    for (rng_pptr = 0; rng_pptr < 256; ++rng_pptr) rng_pool[rng_pptr] = 0;
    rng_pptr = 0;
    rng_ready = 1;
  }
  return arc4_next();
}

/* pkcs1pad2: 0x00 0x02 nonzero-random-padding 0x00 message, as an n-byte block. */
static BI *pkcs1pad2(const char *s, int n) {
  unsigned char ba[256];
  int len = (int)strlen(s);
  int total = n;
  if (n < len + 11) { fprintf(stderr, "Message too long for RSA\n"); exit(1); }
  int i = len - 1;
  while (i >= 0 && n > 0) ba[--n] = (unsigned char)s[i--];
  ba[--n] = 0;
  while (n > 2) { /* random non-zero pad */
    int x0 = 0;
    while (x0 == 0) x0 = rng_get_byte();
    ba[--n] = (unsigned char)x0;
  }
  ba[--n] = 2;
  ba[--n] = 0;
  return bi_from_bytes(ba, total);
}

/* pkcs1unpad2: returns the message length in out, or -1 when the block is malformed. */
static int pkcs1unpad2(const int *b, int blen, int n, unsigned char *out) {
  int i = 0;
  while (i < blen && b[i] == 0) ++i;
  if (blen - i != n - 1 || b[i] != 2) return -1;
  ++i;
  while (b[i] != 0) if (++i >= blen) return -1;
  int len = 0;
  while (++i < blen) out[len++] = (unsigned char)b[i];
  return len;
}

typedef struct {
  BI *n;
  i64 e;
  BI *d, *p, *q, *dmp1, *dmq1, *coeff;
} RSAKey;

static void rsa_set_public(RSAKey *k, const char *N, const char *E) {
  k->n = bi_parse_hex(N);
  k->e = strtol(E, NULL, 16);
}

static void rsa_set_private_ex(RSAKey *k, const char *N, const char *E, const char *D,
                               const char *P, const char *Q, const char *DP, const char *DQ,
                               const char *C) {
  k->n = bi_parse_hex(N);
  k->e = strtol(E, NULL, 16);
  k->d = bi_parse_hex(D);
  k->p = bi_parse_hex(P);
  k->q = bi_parse_hex(Q);
  k->dmp1 = bi_parse_hex(DP);
  k->dmq1 = bi_parse_hex(DQ);
  k->coeff = bi_parse_hex(C);
}

/* RSAEncrypt: writes the ciphertext as a lowercase hex string with an even number of digits. */
static void rsa_encrypt(RSAKey *k, const char *text, char *hexout) {
  int nbytes = (bi_bitLength(k->n) + 7) >> 3;
  BI *m = pkcs1pad2(text, nbytes);
  BI *c = bi_modPowInt(m, k->e, k->n);
  char tmp[1024];
  bi_to_hex(c, tmp);
  if ((strlen(tmp) & 1) == 0) strcpy(hexout, tmp);
  else { hexout[0] = '0'; strcpy(hexout + 1, tmp); }
}

/* RSADoPrivate: CRT exponentiation with Garner recombination. */
static BI *rsa_do_private(RSAKey *k, BI *x) {
  BI *xp = bi_mod(x, k->p);
  xp = bi_modPow(xp, k->dmp1, k->p);
  BI *xq = bi_mod(x, k->q);
  xq = bi_modPow(xq, k->dmq1, k->q);
  while (bi_compareTo(xp, xq) < 0) xp = bi_add(xp, k->p);
  BI *t1 = bi_subtract(xp, xq);
  BI *t2 = bi_multiply(t1, k->coeff);
  BI *t3 = bi_mod(t2, k->p);
  BI *t4 = bi_multiply(t3, k->q);
  return bi_add(t4, xq);
}

/* RSADecrypt: returns the message length in out, or -1 on a malformed block. */
static int rsa_decrypt(RSAKey *k, const char *ctext, unsigned char *out) {
  BI *c = bi_parse_hex(ctext);
  BI *m = rsa_do_private(k, c);
  int nbytes = (bi_bitLength(k->n) + 7) >> 3;
  int b[256];
  int blen = bi_toByteArray(m, b);
  return pkcs1unpad2(b, blen, nbytes, out);
}

static const char TEXT[] =
    "The quick brown fox jumped over the extremely lazy frog! "
    "Now is the time for all good men to come to the party.";
static const int TEXT_LEN = (int)(sizeof TEXT - 1);

static const char *nValue = "a5261939975948bb7a58dffe5ff54e65f0498f9175f5a09288810b8975871e99af3b5dd94057b0fc07535f5f97444504fa35169d461d0d30cf0192e307727c065168c788771c561a9400fb49175e9e6aa4e23fe11af69e9412dd23b0cb6684c4c2429bce139e848ab26d0829073351f4acd36074eafd036a5eb83359d2a698d3";
static const char *eValue = "10001";
static const char *dValue = "8e9912f6d3645894e8d38cb58c0db81ff516cf4c7e5a14c7f1eddb1459d2cded4d8d293fc97aee6aefb861859c8b6a3d1dfe710463e1f9ddc72048c09751971c4a580aa51eb523357a3cc48d31cfad1d4a165066ed92d4748fb6571211da5cb14bc11b6e2df7c1a559e6d5ac1cd5c94703a22891464fba23d0d965086277a161";
static const char *pValue = "d090ce58a92c75233a6486cb0a9209bf3583b64f540c76f5294bb97d285eed33aec220bde14b2417951178ac152ceab6da7090905b478195498b352048f15e7d";
static const char *qValue = "cab575dc652bb66df15a0359609d51d1db184750c00c6698b90ef3465c99655103edbf0d54c56aec0ce3c4d22592338092a126a0cc49f65a4a30d222b411e58f";
static const char *dmp1Value = "1a24bca8e273df2f0e47c199bbf678604e7df7215480c77c8db39f49b000ce2cf7500038acfff5433b7d582a01f1826e6f4d42e1c57f5e1fef7b12aabc59fd25";
static const char *dmq1Value = "3d06982efbbe47339e1f6d36b1216b8a741d410b0c662f54f7118b27b9a4ec9d914337eb39841d8666f3034408cf94f5b62f11c402fc994fe15a05493150d9fd";
static const char *coeffValue = "3a3e731acd8960b7ff9eb81a7ff93bd1cfa74cbd56987db58b4594fb09c09084db1734c8143f98b602b981aaa9243ca28deb69b5b280ee8dcee0fd2625e53250";

static char encrypted[1024];
static long long bytes_checked;

static void encrypt(void) {
  slots_used = 0;
  RSAKey k;
  rsa_set_public(&k, nValue, eValue);
  rsa_set_private_ex(&k, nValue, eValue, dValue, pValue, qValue, dmp1Value, dmq1Value, coeffValue);
  rsa_encrypt(&k, TEXT, encrypted);
}

static void decrypt(void) {
  slots_used = 0;
  RSAKey k;
  rsa_set_public(&k, nValue, eValue);
  rsa_set_private_ex(&k, nValue, eValue, dValue, pValue, qValue, dmp1Value, dmq1Value, coeffValue);
  unsigned char decrypted[256];
  int len = rsa_decrypt(&k, encrypted, decrypted);
  if (len != TEXT_LEN || memcmp(decrypted, TEXT, (size_t)TEXT_LEN) != 0) {
    fprintf(stderr, "Crypto operation failed\n");
    exit(1);
  }
  bytes_checked += len;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  rng_init_pool();
  zero_obj.t = 0; zero_obj.s = 0;
  one_obj.t = 1; one_obj.s = 0; one_obj.a[0] = 1;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) {
    encrypt();
    decrypt();
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("crypto %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  printf("validated %d round trips, %lld plaintext bytes\n", iterations, bytes_checked);
  return 0;
}
