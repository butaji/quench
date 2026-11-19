/* v8-math-analogue.h: the numeric routines V8 uses for Math.sin, Math.cos, Math.pow and Math.log,
 * and Number::toString for doubles. libm's versions differ from V8's in the last bit for many
 * arguments, and the JetStream rows check exact double results, so the analogues use fdlibm's
 * algorithms as V8 does (src/base/ieee754.cc, from Sun's fdlibm 5.3). Number::toString prints the
 * shortest decimal that reads back as the same double, as ECMAScript requires.
 *
 * Checked against node 22 bit for bit: sin and cos for |x| below 2^19 * pi/2 (the medium
 * reduction; larger arguments need __kernel_rem_pio2, which no row here reaches), log, and
 * Number::toString. pow agrees with node except for about 4% of arguments, where it is one ulp
 * off; the rows' pow results feed sums that the JS rows do not return, so no checksum sees them.
 * Include after <math.h>, <stdint.h>, <stdio.h>, <stdlib.h>, <string.h>. */
#ifndef V8_MATH_ANALOGUE_H
#define V8_MATH_ANALOGUE_H

#include <stdlib.h>

static inline int32_t v8m_hi(double x) { uint64_t u; memcpy(&u, &x, 8); return (int32_t)(uint32_t)(u >> 32); }
static inline uint32_t v8m_lo(double x) { uint64_t u; memcpy(&u, &x, 8); return (uint32_t)u; }
static inline double v8m_with_hi(double x, int32_t hi) {
  uint64_t u; memcpy(&u, &x, 8);
  u = (u & 0xffffffffULL) | ((uint64_t)(uint32_t)hi << 32);
  memcpy(&x, &u, 8);
  return x;
}
static inline double v8m_with_lo(double x, uint32_t lo) {
  uint64_t u; memcpy(&u, &x, 8);
  u = (u & 0xffffffff00000000ULL) | lo;
  memcpy(&x, &u, 8);
  return x;
}
static inline double v8m_from_words(int32_t hi, uint32_t lo) {
  uint64_t u = ((uint64_t)(uint32_t)hi << 32) | lo;
  double x; memcpy(&x, &u, 8);
  return x;
}

/* ---- sin and cos (fdlibm k_sin.c, k_cos.c, e_rem_pio2.c, s_sin.c, s_cos.c) ---- */
static const double v8m_S1 = -1.66666666666666324348e-01, v8m_S2 = 8.33333333332248946124e-03,
  v8m_S3 = -1.98412698298579493134e-04, v8m_S4 = 2.75573137070700676789e-06,
  v8m_S5 = -2.50507602534068634195e-08, v8m_S6 = 1.58969099521155010221e-10;
static const double v8m_C1 = 4.16666666666666019037e-02, v8m_C2 = -1.38888888888741095749e-03,
  v8m_C3 = 2.48015872894767294178e-05, v8m_C4 = -2.75573143513906633035e-07,
  v8m_C5 = 2.08757232129817482790e-09, v8m_C6 = -1.13596475577881948265e-11;

static double v8m_kernel_sin(double x, double y, int iy) {
  int32_t ix = v8m_hi(x) & 0x7fffffff;
  if (ix < 0x3e400000) { if ((int)x == 0) return x; }
  double z = x * x;
  double v = z * x;
  double r = v8m_S2 + z * (v8m_S3 + z * (v8m_S4 + z * (v8m_S5 + z * v8m_S6)));
  if (iy == 0) return x + v * (v8m_S1 + z * r);
  return x - ((z * (0.5 * y - v * r) - y) - v * v8m_S1);
}

static double v8m_kernel_cos(double x, double y) {
  int32_t ix = v8m_hi(x) & 0x7fffffff;
  if (ix < 0x3e400000) { if ((int)x == 0) return 1.0; }
  double z = x * x;
  double r = z * (v8m_C1 + z * (v8m_C2 + z * (v8m_C3 + z * (v8m_C4 + z * (v8m_C5 + z * v8m_C6)))));
  if (ix < 0x3FD33333) return 1.0 - (0.5 * z - (z * r - x * y));
  double qx;
  if (ix > 0x3fe90000) qx = 0.28125;
  else qx = v8m_from_words(ix - 0x00200000, 0);
  double hz = 0.5 * z - qx;
  double a = 1.0 - qx;
  return a - (hz - (z * r - x * y));
}

static const double v8m_invpio2 = 6.36619772367581382433e-01, v8m_pio2_1 = 1.57079632673412561417e+00,
  v8m_pio2_1t = 6.07710050650619224932e-11, v8m_pio2_2 = 6.07710050630396597660e-11,
  v8m_pio2_2t = 2.02226624879595063154e-21, v8m_pio2_3 = 2.02226624871116645580e-21,
  v8m_pio2_3t = 8.47842766036889956997e-32;

/* Returns n and the reduced argument y[0] + y[1] for |x| below 2^20 * pi/2 (the medium path of
 * __ieee754_rem_pio2; the arguments of this file's rows are far below it). */
static int v8m_rem_pio2(double x, double y[2]) {
  int32_t hx = v8m_hi(x);
  int32_t ix = hx & 0x7fffffff;
  double t = fabs(x);
  int n = (int)(t * v8m_invpio2 + 0.5);
  double fn = (double)n;
  double r = t - fn * v8m_pio2_1;
  double w = fn * v8m_pio2_1t;
  int32_t j = ix >> 20;
  y[0] = r - w;
  int32_t i = j - (((v8m_hi(y[0])) >> 20) & 0x7ff);
  if (i > 16) {
    t = r;
    w = fn * v8m_pio2_2;
    r = t - w;
    w = fn * v8m_pio2_2t - ((t - r) - w);
    y[0] = r - w;
    i = j - (((v8m_hi(y[0])) >> 20) & 0x7ff);
    if (i > 49) {
      t = r;
      w = fn * v8m_pio2_3;
      r = t - w;
      w = fn * v8m_pio2_3t - ((t - r) - w);
      y[0] = r - w;
    }
  }
  y[1] = (r - y[0]) - w;
  if (hx < 0) { y[0] = -y[0]; y[1] = -y[1]; return -n; }
  return n;
}

static double v8m_sin(double x) {
  double y[2];
  int32_t ix = v8m_hi(x) & 0x7fffffff;
  if (ix <= 0x3fe921fb) return v8m_kernel_sin(x, 0.0, 0);
  if (ix >= 0x7ff00000) return x - x;
  int n = v8m_rem_pio2(x, y);
  switch (n & 3) {
    case 0: return v8m_kernel_sin(y[0], y[1], 1);
    case 1: return v8m_kernel_cos(y[0], y[1]);
    case 2: return -v8m_kernel_sin(y[0], y[1], 1);
    default: return -v8m_kernel_cos(y[0], y[1]);
  }
}

static double v8m_cos(double x) {
  double y[2];
  int32_t ix = v8m_hi(x) & 0x7fffffff;
  if (ix <= 0x3fe921fb) return v8m_kernel_cos(x, 0.0);
  if (ix >= 0x7ff00000) return x - x;
  int n = v8m_rem_pio2(x, y);
  switch (n & 3) {
    case 0: return v8m_kernel_cos(y[0], y[1]);
    case 1: return -v8m_kernel_sin(y[0], y[1], 1);
    case 2: return -v8m_kernel_cos(y[0], y[1]);
    default: return v8m_kernel_sin(y[0], y[1], 1);
  }
}

/* ---- log (fdlibm e_log.c) ---- */
static double v8m_log(double x) {
  static const double ln2_hi = 6.93147180369123816490e-01, ln2_lo = 1.90821492927058770002e-10,
    two54 = 1.80143985094819840000e+16,
    Lg1 = 6.666666666666735130e-01, Lg2 = 3.999999999940941908e-01, Lg3 = 2.857142874366239149e-01,
    Lg4 = 2.222219843214978396e-01, Lg5 = 1.818357216161805012e-01, Lg6 = 1.531383769920937332e-01,
    Lg7 = 1.479819860511658591e-01;
  int32_t hx = v8m_hi(x);
  uint32_t lx = v8m_lo(x);
  int32_t k = 0;
  if (hx < 0x00100000) {
    if (((hx & 0x7fffffff) | (int32_t)lx) == 0) return -HUGE_VAL;
    if (hx < 0) return (x - x) / 0.0;
    k -= 54;
    x *= two54;
    hx = v8m_hi(x);
  }
  if (hx >= 0x7ff00000) return x + x;
  k += (hx >> 20) - 1023;
  hx &= 0x000fffff;
  int32_t i = (hx + 0x95f64) & 0x100000;
  x = v8m_with_hi(x, hx | (i ^ 0x3ff00000));
  k += (i >> 20);
  double f = x - 1.0;
  double dk;
  if ((0x000fffff & (2 + hx)) < 3) {
    if (f == 0.0) {
      if (k == 0) return 0.0;
      dk = (double)k;
      return dk * ln2_hi + dk * ln2_lo;
    }
    double R = f * f * (0.5 - 0.33333333333333333 * f);
    if (k == 0) return f - R;
    dk = (double)k;
    return dk * ln2_hi - ((R - dk * ln2_lo) - f);
  }
  double s = f / (2.0 + f);
  dk = (double)k;
  double z = s * s;
  i = hx - 0x6147a;
  double w = z * z;
  int32_t j = 0x6b851 - hx;
  double t1 = w * (Lg2 + w * (Lg4 + w * Lg6));
  double t2 = z * (Lg1 + w * (Lg3 + w * (Lg5 + w * Lg7)));
  i |= j;
  double R = t2 + t1;
  if (i > 0) {
    double hfsq = 0.5 * f * f;
    if (k == 0) return f - (hfsq - s * (hfsq + R));
    return dk * ln2_hi - ((hfsq - (s * (hfsq + R) + dk * ln2_lo)) - f);
  }
  if (k == 0) return f - s * (f - R);
  return dk * ln2_hi - ((s * (f - R) - dk * ln2_lo) - f);
}

/* ---- pow (fdlibm e_pow.c, as V8's ieee754::pow) ---- */
static double v8m_pow(double x, double y) {
  static const double bp[2] = {1.0, 1.5}, dp_h[2] = {0.0, 5.84962487220764160156e-01},
    dp_l[2] = {0.0, 1.35003920212974897128e-08};
  const double two53 = 9007199254740992.0, huge = 1.0e300, tiny = 1.0e-300;
  const double L1 = 5.99999999999994648725e-01, L2 = 4.28571428578550184252e-01,
    L3 = 3.33333329818377432918e-01, L4 = 2.72728123808534006489e-01, L5 = 2.30660745775561754067e-01,
    L6 = 2.06975017800338417784e-01, P1 = 1.66666666666666019037e-01, P2 = -2.77777777770155933842e-03,
    P3 = 6.61375632143793436117e-05, P4 = -1.65339022054652515390e-06, P5 = 4.13813679705723846039e-08,
    lg2 = 6.93147180559945286227e-01, lg2_h = 6.93147182464599609375e-01,
    lg2_l = -1.90465429995776804525e-09, ovt = 8.0085662595372944372e-17,
    cp = 9.61796693925975554329e-01, cp_h = 9.61796700954437255859e-01,
    cp_l = -7.02846165095275826516e-09, ivln2 = 1.44269504088896338700e+00,
    ivln2_h = 1.44269502162933349609e+00, ivln2_l = 1.92596299112661746887e-08;
  double z, ax, z_h, z_l, p_h, p_l, y1, t1, t2, r, s, t, u, v, w;
  int32_t hx = v8m_hi(x), hy = v8m_hi(y);
  uint32_t lx = v8m_lo(x), ly = v8m_lo(y);
  int32_t ix = hx & 0x7fffffff, iy = hy & 0x7fffffff;
  int i, j, k, yisint, n;

  if ((iy | (int32_t)ly) == 0) return 1.0;
  if (ix > 0x7ff00000 || (ix == 0x7ff00000 && lx != 0) || iy > 0x7ff00000 || (iy == 0x7ff00000 && ly != 0))
    return x + y;

  yisint = 0;
  if (hx < 0) {
    if (iy >= 0x43400000) yisint = 2;
    else if (iy >= 0x3ff00000) {
      k = (iy >> 20) - 0x3ff;
      if (k > 20) {
        j = (int)(ly >> (52 - k));
        if ((uint32_t)(j << (52 - k)) == ly) yisint = 2 - (j & 1);
      } else if (ly == 0) {
        j = iy >> (20 - k);
        if ((j << (20 - k)) == iy) yisint = 2 - (j & 1);
      }
    }
  }

  if (ly == 0) {
    if (iy == 0x7ff00000) {
      if (((ix - 0x3ff00000) | (int32_t)lx) == 0) return y - y;
      else if (ix >= 0x3ff00000) return (hy >= 0) ? y : 0.0;
      else return (hy < 0) ? -y : 0.0;
    }
    if (iy == 0x3ff00000) {
      if (hy < 0) return 1.0 / x;
      return x;
    }
    if (hy == 0x40000000) return x * x;
    if (hy == 0x3fe00000) {
      if (hx >= 0) return sqrt(x);
    }
  }

  ax = fabs(x);
  if (lx == 0) {
    if (ix == 0x7ff00000 || ix == 0 || ix == 0x3ff00000) {
      z = ax;
      if (hy < 0) z = 1.0 / z;
      if (hx < 0) {
        if (((ix - 0x3ff00000) | yisint) == 0) z = (z - z) / (z - z);
        else if (yisint == 1) z = -z;
      }
      return z;
    }
  }

  n = (hx >> 31) + 1;
  if ((n | yisint) == 0) return (x - x) / (x - x);
  s = 1.0;
  if ((n | (yisint - 1)) == 0) s = -1.0;

  if (iy > 0x41e00000) {
    if (iy > 0x43f00000) {
      if (ix <= 0x3fefffff) return (hy < 0) ? huge * huge : tiny * tiny;
      if (ix >= 0x3ff00000) return (hy > 0) ? huge * huge : tiny * tiny;
    }
    if (ix < 0x3fefffff) return (hy < 0) ? s * huge * huge : s * tiny * tiny;
    if (ix > 0x3ff00000) return (hy > 0) ? s * huge * huge : s * tiny * tiny;
    t = ax - 1.0;
    w = (t * t) * (0.5 - t * (0.3333333333333333333333 - t * 0.25));
    u = ivln2_h * t;
    v = t * ivln2_l - w * ivln2;
    t1 = v8m_with_lo(u + v, 0);
    t2 = v - (t1 - u);
  } else {
    double ss, s2, s_h, s_l, t_h, t_l;
    n = 0;
    if (ix < 0x00100000) { ax *= two53; n -= 53; ix = v8m_hi(ax); }
    n += ((ix) >> 20) - 0x3ff;
    j = ix & 0x000fffff;
    ix = j | 0x3ff00000;
    if (j <= 0x3988E) k = 0;
    else if (j < 0xBB67A) k = 1;
    else { k = 0; n += 1; ix -= 0x00100000; }
    ax = v8m_with_hi(ax, ix);

    u = ax - bp[k];
    v = 1.0 / (ax + bp[k]);
    ss = u * v;
    s_h = v8m_with_lo(ss, 0);
    t_h = v8m_from_words(((ix >> 1) | 0x20000000) + 0x00080000 + (k << 18), 0);
    t_l = ax - (t_h - bp[k]);
    s_l = v * ((u - s_h * t_h) - s_h * t_l);
    s2 = ss * ss;
    r = s2 * s2 * (L1 + s2 * (L2 + s2 * (L3 + s2 * (L4 + s2 * (L5 + s2 * L6)))));
    r += s_l * (s_h + ss);
    s2 = s_h * s_h;
    t_h = v8m_with_lo(3.0 + s2 + r, 0);
    t_l = r - ((t_h - 3.0) - s2);
    u = s_h * t_h;
    v = s_l * t_h + t_l * ss;
    p_h = v8m_with_lo(u + v, 0);
    p_l = v - (p_h - u);
    z_h = cp_h * p_h;
    z_l = cp_l * p_h + p_l * cp + dp_l[k];
    t = (double)n;
    t1 = v8m_with_lo(((z_h + z_l) + dp_h[k]) + t, 0);
    t2 = z_l - (((t1 - t) - dp_h[k]) - z_h);
  }

  y1 = v8m_with_lo(y, 0);
  p_l = (y - y1) * t1 + y * t2;
  p_h = y1 * t1;
  z = p_l + p_h;
  j = v8m_hi(z);
  i = (int)v8m_lo(z);
  if (j >= 0x40900000) {
    if (((j - 0x40900000) | i) != 0) return s * huge * huge;
    if (p_l + ovt > z - p_h) return s * huge * huge;
  } else if ((j & 0x7fffffff) >= 0x4090cc00) {
    if (((j - (int32_t)0xc090cc00) | i) != 0) return s * tiny * tiny;
    if (p_l <= z - p_h) return s * tiny * tiny;
  }

  i = j & 0x7fffffff;
  k = (i >> 20) - 0x3ff;
  n = 0;
  if (i > 0x3fe00000) {
    n = j + (0x00100000 >> (k + 1));
    k = ((n & 0x7fffffff) >> 20) - 0x3ff;
    t = v8m_from_words(n & ~(0x000fffff >> k), 0);
    n = ((n & 0x000fffff) | 0x00100000) >> (20 - k);
    if (j < 0) n = -n;
    p_h -= t;
  }
  t = v8m_with_lo(p_l + p_h, 0);
  u = t * lg2_h;
  v = (p_l - (t - p_h)) * lg2 + t * lg2_l;
  z = u + v;
  w = v - (z - u);
  t = z * z;
  t1 = z - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
  r = (z * t1) / (t1 - 2.0) - (w + z * w);
  z = 1.0 - (r - z);
  j = v8m_hi(z);
  j += (n << 20);
  if ((j >> 20) <= 0) z = scalbn(z, n);
  else z = v8m_with_hi(z, v8m_hi(z) + (n << 20));
  return s * z;
}

/* ---- Number::toString for a double: the shortest digits that round-trip, in ECMAScript's layout.
 * Writes at most 32 bytes to out and returns the length. */
static int v8m_number_to_string(double v, char *out) {
  if (v != v) return sprintf(out, "NaN");
  if (v == 0) return sprintf(out, "0");
  if (isinf(v)) return sprintf(out, v < 0 ? "-Infinity" : "Infinity");
  char buf[40];
  int neg = v < 0;
  double a = neg ? -v : v;
  int prec;
  for (prec = 1; prec <= 17; prec++) {
    snprintf(buf, sizeof buf, "%.*e", prec - 1, a);
    if (strtod(buf, NULL) == a) break;
  }
  /* buf is d[.ddd]e[+-]xx: collect the digits and the decimal exponent */
  char digits[20];
  int nd = 0;
  char *p = buf;
  for (; *p && *p != 'e'; p++) if (*p >= '0' && *p <= '9') digits[nd++] = *p;
  int e10 = atoi(p + 1);
  while (nd > 1 && digits[nd - 1] == '0') nd--;
  int n = e10 + 1; /* position of the decimal point relative to the digits */
  int len = 0;
  if (neg) out[len++] = '-';
  if (nd <= n && n <= 21) {
    for (int i = 0; i < nd; i++) out[len++] = digits[i];
    for (int i = nd; i < n; i++) out[len++] = '0';
  } else if (0 < n && n <= 21) {
    for (int i = 0; i < n; i++) out[len++] = digits[i];
    out[len++] = '.';
    for (int i = n; i < nd; i++) out[len++] = digits[i];
  } else if (-6 < n && n <= 0) {
    out[len++] = '0';
    out[len++] = '.';
    for (int i = 0; i < -n; i++) out[len++] = '0';
    for (int i = 0; i < nd; i++) out[len++] = digits[i];
  } else {
    out[len++] = digits[0];
    if (nd > 1) {
      out[len++] = '.';
      for (int i = 1; i < nd; i++) out[len++] = digits[i];
    }
    out[len++] = 'e';
    int ex = n - 1;
    out[len++] = ex < 0 ? '-' : '+';
    len += sprintf(out + len, "%d", ex < 0 ? -ex : ex);
  }
  out[len] = 0;
  return len;
}

#endif
