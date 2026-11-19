// jetstream-row: bigint-noble-ed25519
// jetstream-files: bigint/web-crypto-sham.js bigint/noble-ed25519-bundle.js bigint/noble-benchmark.js
// iterations: 30
/* C analogue of JetStream's bigint/noble-benchmark.js with noble-ed25519 v1 (bundle). One iteration
 * runs the same sequence of operations as runNobleBenchmark: precompute of the base-point window table
 * (rebuilt each iteration, as the JS _setWindowSize deletes the cache), 30 getPublicKey with the
 * fixed key and 30 with random keys, 20 sign, 4 verify on hex inputs, Signature/Point objects and 5
 * verify, 65 Point.fromHex, 30 Ristretto hashToCurve, 30 Ristretto decode and encode, 6 x25519 base
 * scalar multiplications, 5 getSharedSecret, and the same counts again for the sync API.
 * Field arithmetic is a plain multiprecision implementation: four 64-bit limbs modulo 2^255-19 with a
 * portable 32x32->64 product, and 512-bit integers (16 x 32-bit limbs) for scalars, with binary
 * long division for reduction. SHA-512 is FIPS 180-4; its round constants are the ones in the bundle.
 * Deviations from noble: inversion is the ref10 Fermat chain (noble uses extended Euclid over BigInt),
 * point decode and encode work on bytes without hex strings, and async/Promise wrappers are gone.
 * Validation (untimed, after the loop): the public key of the RFC 8032 test-1 secret key must be
 * d75a9801...511a, a signature must verify, and a Ristretto encoding must round-trip.
 * Usage: bigint-noble-ed25519 [iterations] */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef uint8_t u8;
typedef uint32_t u32;
typedef uint64_t u64;

static void die(const char *msg) {
  fprintf(stderr, "%s\n", msg);
  exit(1);
}

/* ---------- SHA-512 (FIPS 180-4) ---------- */

static const u64 SHA512_K[80] = {
  0x428a2f98d728ae22ULL,
  0x7137449123ef65cdULL,
  0xb5c0fbcfec4d3b2fULL,
  0xe9b5dba58189dbbcULL,
  0x3956c25bf348b538ULL,
  0x59f111f1b605d019ULL,
  0x923f82a4af194f9bULL,
  0xab1c5ed5da6d8118ULL,
  0xd807aa98a3030242ULL,
  0x12835b0145706fbeULL,
  0x243185be4ee4b28cULL,
  0x550c7dc3d5ffb4e2ULL,
  0x72be5d74f27b896fULL,
  0x80deb1fe3b1696b1ULL,
  0x9bdc06a725c71235ULL,
  0xc19bf174cf692694ULL,
  0xe49b69c19ef14ad2ULL,
  0xefbe4786384f25e3ULL,
  0x0fc19dc68b8cd5b5ULL,
  0x240ca1cc77ac9c65ULL,
  0x2de92c6f592b0275ULL,
  0x4a7484aa6ea6e483ULL,
  0x5cb0a9dcbd41fbd4ULL,
  0x76f988da831153b5ULL,
  0x983e5152ee66dfabULL,
  0xa831c66d2db43210ULL,
  0xb00327c898fb213fULL,
  0xbf597fc7beef0ee4ULL,
  0xc6e00bf33da88fc2ULL,
  0xd5a79147930aa725ULL,
  0x06ca6351e003826fULL,
  0x142929670a0e6e70ULL,
  0x27b70a8546d22ffcULL,
  0x2e1b21385c26c926ULL,
  0x4d2c6dfc5ac42aedULL,
  0x53380d139d95b3dfULL,
  0x650a73548baf63deULL,
  0x766a0abb3c77b2a8ULL,
  0x81c2c92e47edaee6ULL,
  0x92722c851482353bULL,
  0xa2bfe8a14cf10364ULL,
  0xa81a664bbc423001ULL,
  0xc24b8b70d0f89791ULL,
  0xc76c51a30654be30ULL,
  0xd192e819d6ef5218ULL,
  0xd69906245565a910ULL,
  0xf40e35855771202aULL,
  0x106aa07032bbd1b8ULL,
  0x19a4c116b8d2d0c8ULL,
  0x1e376c085141ab53ULL,
  0x2748774cdf8eeb99ULL,
  0x34b0bcb5e19b48a8ULL,
  0x391c0cb3c5c95a63ULL,
  0x4ed8aa4ae3418acbULL,
  0x5b9cca4f7763e373ULL,
  0x682e6ff3d6b2b8a3ULL,
  0x748f82ee5defb2fcULL,
  0x78a5636f43172f60ULL,
  0x84c87814a1f0ab72ULL,
  0x8cc702081a6439ecULL,
  0x90befffa23631e28ULL,
  0xa4506cebde82bde9ULL,
  0xbef9a3f7b2c67915ULL,
  0xc67178f2e372532bULL,
  0xca273eceea26619cULL,
  0xd186b8c721c0c207ULL,
  0xeada7dd6cde0eb1eULL,
  0xf57d4f7fee6ed178ULL,
  0x06f067aa72176fbaULL,
  0x0a637dc5a2c898a6ULL,
  0x113f9804bef90daeULL,
  0x1b710b35131c471bULL,
  0x28db77f523047d84ULL,
  0x32caab7b40c72493ULL,
  0x3c9ebe0a15c9bebcULL,
  0x431d67c49c100d4cULL,
  0x4cc5d4becb3e42b6ULL,
  0x597f299cfc657e2aULL,
  0x5fcb6fab3ad6faecULL,
  0x6c44198c4a475817ULL,
};

typedef struct { u64 h[8]; u8 buf[128]; size_t buflen; u64 total; } Sha512;

static u64 rotr64(u64 x, int n) { return (x >> n) | (x << (64 - n)); }

static void sha512_init(Sha512 *c) {
  static const u64 IV[8] = {0x6a09e667f3bcc908ULL, 0xbb67ae8584caa73bULL, 0x3c6ef372fe94f82bULL,
                            0xa54ff53a5f1d36f1ULL, 0x510e527fade682d1ULL, 0x9b05688c2b3e6c1fULL,
                            0x1f83d9abfb41bd6bULL, 0x5be0cd19137e2179ULL};
  memcpy(c->h, IV, sizeof IV);
  c->buflen = 0;
  c->total = 0;
}

static void sha512_block(Sha512 *c, const u8 *p) {
  u64 w[80];
  for (int i = 0; i < 16; i++) {
    w[i] = 0;
    for (int k = 0; k < 8; k++) w[i] = (w[i] << 8) | p[i * 8 + k];
  }
  for (int i = 16; i < 80; i++) {
    u64 s0 = rotr64(w[i - 15], 1) ^ rotr64(w[i - 15], 8) ^ (w[i - 15] >> 7);
    u64 s1 = rotr64(w[i - 2], 19) ^ rotr64(w[i - 2], 61) ^ (w[i - 2] >> 6);
    w[i] = w[i - 16] + s0 + w[i - 7] + s1;
  }
  u64 a = c->h[0], b = c->h[1], cc = c->h[2], d = c->h[3], e = c->h[4], f = c->h[5], g = c->h[6], h = c->h[7];
  for (int i = 0; i < 80; i++) {
    u64 S1 = rotr64(e, 14) ^ rotr64(e, 18) ^ rotr64(e, 41);
    u64 ch = (e & f) ^ (~e & g);
    u64 t1 = h + S1 + ch + SHA512_K[i] + w[i];
    u64 S0 = rotr64(a, 28) ^ rotr64(a, 34) ^ rotr64(a, 39);
    u64 maj = (a & b) ^ (a & cc) ^ (b & cc);
    u64 t2 = S0 + maj;
    h = g;
    g = f;
    f = e;
    e = d + t1;
    d = cc;
    cc = b;
    b = a;
    a = t1 + t2;
  }
  c->h[0] += a;
  c->h[1] += b;
  c->h[2] += cc;
  c->h[3] += d;
  c->h[4] += e;
  c->h[5] += f;
  c->h[6] += g;
  c->h[7] += h;
}

static void sha512_update(Sha512 *c, const u8 *p, size_t n) {
  c->total += n;
  while (n) {
    size_t take = 128 - c->buflen;
    if (take > n) take = n;
    memcpy(c->buf + c->buflen, p, take);
    c->buflen += take;
    p += take;
    n -= take;
    if (c->buflen == 128) {
      sha512_block(c, c->buf);
      c->buflen = 0;
    }
  }
}

static void sha512_final(Sha512 *c, u8 out[64]) {
  u64 bits = c->total * 8;
  u8 pad = 0x80;
  sha512_update(c, &pad, 1);
  u8 zero = 0;
  while (c->buflen != 112) sha512_update(c, &zero, 1);
  u8 len[16] = {0};
  for (int i = 0; i < 8; i++) len[15 - i] = (u8)(bits >> (8 * i));
  sha512_update(c, len, 16);
  for (int i = 0; i < 8; i++)
    for (int k = 0; k < 8; k++) out[i * 8 + k] = (u8)(c->h[i] >> (56 - 8 * k));
}

/* hash of the concatenation of up to three byte strings */
static void sha512_cat(u8 out[64], const u8 *a, size_t na, const u8 *b, size_t nb, const u8 *c3, size_t nc) {
  Sha512 c;
  sha512_init(&c);
  if (na) sha512_update(&c, a, na);
  if (nb) sha512_update(&c, b, nb);
  if (nc) sha512_update(&c, c3, nc);
  sha512_final(&c, out);
}

/* ---------- 512-bit unsigned integers (scalars and the group order) ---------- */

#define BN 16
typedef struct { u32 w[BN]; } Big;

static void big_zero(Big *a) { memset(a->w, 0, sizeof a->w); }

static void big_from_bytes(Big *r, const u8 *b, size_t n) {
  big_zero(r);
  for (size_t i = 0; i < n; i++) r->w[i / 4] |= (u32)b[i] << (8 * (i % 4));
}

static void big_to_bytes(const Big *a, u8 *out, size_t n) {
  for (size_t i = 0; i < n; i++) out[i] = (u8)(a->w[i / 4] >> (8 * (i % 4)));
}

static void big_from_dec(Big *r, const char *s) {
  big_zero(r);
  for (; *s; s++) {
    u64 c = (u64)(*s - '0');
    for (int i = 0; i < BN; i++) {
      u64 cur = (u64)r->w[i] * 10 + c;
      r->w[i] = (u32)cur;
      c = cur >> 32;
    }
  }
}

static int big_cmp(const Big *a, const Big *b) {
  for (int i = BN - 1; i >= 0; i--)
    if (a->w[i] != b->w[i]) return a->w[i] > b->w[i] ? 1 : -1;
  return 0;
}

static int big_is_zero(const Big *a) {
  for (int i = 0; i < BN; i++)
    if (a->w[i]) return 0;
  return 1;
}

static int big_bitlen(const Big *a) {
  for (int i = BN - 1; i >= 0; i--)
    if (a->w[i]) {
      int b = 31;
      while (!((a->w[i] >> b) & 1)) b--;
      return i * 32 + b + 1;
    }
  return 0;
}

static int big_bit(const Big *a, int i) { return (int)((a->w[i / 32] >> (i % 32)) & 1); }

static void big_add(Big *r, const Big *a, const Big *b) {
  u64 c = 0;
  for (int i = 0; i < BN; i++) {
    u64 s = (u64)a->w[i] + b->w[i] + c;
    r->w[i] = (u32)s;
    c = s >> 32;
  }
}

static void big_sub(Big *r, const Big *a, const Big *b) {
  u64 br = 0;
  for (int i = 0; i < BN; i++) {
    u64 d = (u64)a->w[i] - b->w[i] - br;
    r->w[i] = (u32)d;
    br = (d >> 63) & 1;
  }
}

/* r = a * b, truncated to BN limbs (callers keep the product below 2^512) */
static void big_mul(Big *r, const Big *a, const Big *b) {
  Big t;
  big_zero(&t);
  for (int i = 0; i < BN; i++) {
    u64 c = 0;
    for (int j = 0; i + j < BN; j++) {
      u64 cur = (u64)t.w[i + j] + (u64)a->w[i] * b->w[j] + c;
      t.w[i + j] = (u32)cur;
      c = cur >> 32;
    }
  }
  *r = t;
}

/* r = x mod m by binary long division over the bit length of x */
static void big_mod(Big *r, const Big *x, const Big *m) {
  Big rem;
  big_zero(&rem);
  for (int i = big_bitlen(x) - 1; i >= 0; i--) {
    u32 carry = (u32)big_bit(x, i);
    for (int k = 0; k < BN; k++) {
      u32 nw = (rem.w[k] << 1) | carry;
      carry = rem.w[k] >> 31;
      rem.w[k] = nw;
    }
    if (big_cmp(&rem, m) >= 0) big_sub(&rem, &rem, m);
  }
  *r = rem;
}

static Big BIG_L; /* the group order 2^252 + 27742317777372353535851937790883648493 */

/* modlLE: little-endian bytes reduced modulo the group order */
static Big modl_le(const u8 *b, size_t n) {
  Big x, r;
  big_from_bytes(&x, b, n);
  big_mod(&r, &x, &BIG_L);
  return r;
}

/* ---------- field GF(2^255 - 19), four 64-bit limbs, always canonical ---------- */

typedef struct { u64 v[4]; } Fe;
static const u64 P_LIMB[4] = {0xffffffffffffffedULL, 0xffffffffffffffffULL, 0xffffffffffffffffULL,
                              0x7fffffffffffffffULL};

static Fe FE_ZERO = {{0, 0, 0, 0}};
static Fe FE_ONE = {{1, 0, 0, 0}};
static Fe FE_D, FE_SQRT_M1, FE_SQRT_AD_MINUS_ONE, FE_INVSQRT_A_MINUS_D, FE_ONE_MINUS_D_SQ, FE_D_MINUS_ONE_SQ;
static Fe FE_GX, FE_GY;

static void mul64(u64 a, u64 b, u64 *hi, u64 *lo) {
  u64 a0 = (u32)a, a1 = a >> 32, b0 = (u32)b, b1 = b >> 32;
  u64 p00 = a0 * b0, p01 = a0 * b1, p10 = a1 * b0, p11 = a1 * b1;
  u64 mid = (p00 >> 32) + (u32)p01 + (u32)p10;
  *lo = (mid << 32) | (u32)p00;
  *hi = p11 + (p01 >> 32) + (p10 >> 32) + (mid >> 32);
}

static int fe_ge_p(const Fe *a) {
  for (int i = 3; i >= 0; i--) {
    if (a->v[i] > P_LIMB[i]) return 1;
    if (a->v[i] < P_LIMB[i]) return 0;
  }
  return 1;
}

static void fe_sub_p(Fe *a) {
  u64 b = 0;
  for (int i = 0; i < 4; i++) {
    u64 x = a->v[i], y = P_LIMB[i];
    u64 d = x - y;
    u64 b1 = x < y;
    u64 d2 = d - b;
    u64 b2 = d < b;
    a->v[i] = d2;
    b = b1 | b2;
  }
}

static void fe_canon(Fe *a) {
  for (int k = 0; k < 2; k++)
    if (fe_ge_p(a)) fe_sub_p(a);
}

/* r += k, with 2^256 folded back as 38 */
static void fe_add_small(Fe *r, u64 k) {
  u64 c = k;
  for (int i = 0; i < 4; i++) {
    u64 s = r->v[i] + c;
    c = s < r->v[i];
    r->v[i] = s;
    if (!c) break;
  }
  if (c) fe_add_small(r, 38);
}

static void fe_sub_small(Fe *r, u64 k) {
  u64 b = k;
  for (int i = 0; i < 4 && b; i++) {
    u64 x = r->v[i];
    r->v[i] = x - b;
    b = x < b;
  }
}

static Fe fe_add(Fe a, Fe b) {
  Fe r;
  u64 c = 0;
  for (int i = 0; i < 4; i++) {
    u64 s = a.v[i] + b.v[i];
    u64 c1 = s < a.v[i];
    u64 s2 = s + c;
    u64 c2 = s2 < s;
    r.v[i] = s2;
    c = c1 | c2;
  }
  if (c) fe_add_small(&r, 38);
  fe_canon(&r);
  return r;
}

static Fe fe_sub(Fe a, Fe b) {
  Fe r;
  u64 br = 0;
  for (int i = 0; i < 4; i++) {
    u64 x = a.v[i], y = b.v[i];
    u64 d = x - y;
    u64 b1 = x < y;
    u64 d2 = d - br;
    u64 b2 = d < br;
    r.v[i] = d2;
    br = b1 | b2;
  }
  if (br) fe_sub_small(&r, 38);
  fe_canon(&r);
  return r;
}

static Fe fe_neg(Fe a) { return fe_sub(FE_ZERO, a); }

static Fe fe_mul(Fe a, Fe b) {
  u64 t[8] = {0, 0, 0, 0, 0, 0, 0, 0};
  for (int i = 0; i < 4; i++) {
    u64 carry = 0;
    for (int j = 0; j < 4; j++) {
      u64 hi, lo;
      mul64(a.v[i], b.v[j], &hi, &lo);
      u64 s = t[i + j] + lo;
      hi += s < lo;
      s += carry;
      hi += s < carry;
      t[i + j] = s;
      carry = hi;
    }
    t[i + 4] = carry;
  }
  /* value = low + 2^256 * high, and 2^256 == 38 (mod p) */
  Fe r;
  u64 c = 0;
  for (int i = 0; i < 4; i++) {
    u64 hi, lo;
    mul64(t[i + 4], 38, &hi, &lo);
    u64 s = lo + c;
    hi += s < c;
    u64 s2 = s + t[i];
    hi += s2 < s;
    r.v[i] = s2;
    c = hi;
  }
  fe_add_small(&r, 38 * c);
  fe_canon(&r);
  return r;
}

static Fe fe_sq(Fe a) { return fe_mul(a, a); }

static Fe fe_pow2k(Fe x, int k) {
  for (int i = 0; i < k; i++) x = fe_sq(x);
  return x;
}

static int fe_eq(const Fe *a, const Fe *b) { return memcmp(a->v, b->v, sizeof a->v) == 0; }
static int fe_is_zero(const Fe *a) { return a->v[0] == 0 && a->v[1] == 0 && a->v[2] == 0 && a->v[3] == 0; }
static int fe_is_negative(const Fe *a) { return (int)(a->v[0] & 1); }

/* bytes -> field element with reduction mod p (the 256-bit input is taken as is) */
static Fe fe_from_bytes(const u8 *b) {
  Fe r;
  for (int i = 0; i < 4; i++) {
    r.v[i] = 0;
    for (int k = 0; k < 8; k++) r.v[i] |= (u64)b[i * 8 + k] << (8 * k);
  }
  fe_canon(&r);
  fe_canon(&r);
  return r;
}

static void fe_to_bytes(Fe a, u8 *out) {
  fe_canon(&a);
  for (int i = 0; i < 4; i++)
    for (int k = 0; k < 8; k++) out[i * 8 + k] = (u8)(a.v[i] >> (8 * k));
}

static Fe fe_from_u64(u64 x) {
  Fe r = {{x, 0, 0, 0}};
  return r;
}

static Fe fe_from_dec(const char *s) {
  Fe r = FE_ZERO;
  Fe ten = fe_from_u64(10);
  for (; *s; s++) r = fe_add(fe_mul(r, ten), fe_from_u64((u64)(*s - '0')));
  return r;
}

/* x^(2^252 - 3), as the pow_p_5_8 output of noble's pow_2_252_3 */
static Fe fe_pow22523(Fe x) {
  Fe x2 = fe_sq(x);
  Fe b2 = fe_mul(x2, x);
  Fe b4 = fe_mul(fe_pow2k(b2, 2), b2);
  Fe b5 = fe_mul(fe_pow2k(b4, 1), x);
  Fe b10 = fe_mul(fe_pow2k(b5, 5), b5);
  Fe b20 = fe_mul(fe_pow2k(b10, 10), b10);
  Fe b40 = fe_mul(fe_pow2k(b20, 20), b20);
  Fe b80 = fe_mul(fe_pow2k(b40, 40), b40);
  Fe b160 = fe_mul(fe_pow2k(b80, 80), b80);
  Fe b240 = fe_mul(fe_pow2k(b160, 80), b80);
  Fe b250 = fe_mul(fe_pow2k(b240, 10), b10);
  Fe pow = fe_mul(fe_pow2k(b250, 2), x);
  return pow;
}

/* z^(p-2) with the ref10 addition chain; the z == 1 case is the fast path of extended Euclid */
static Fe fe_invert(Fe z) {
  if (fe_eq(&z, &FE_ONE)) return FE_ONE;
  Fe t0 = fe_sq(z);
  Fe t1 = fe_sq(fe_sq(t0));
  t1 = fe_mul(z, t1);
  t0 = fe_mul(t0, t1);
  Fe t2 = fe_sq(t0);
  t1 = fe_mul(t1, t2);
  t2 = fe_pow2k(t1, 5);
  t1 = fe_mul(t2, t1);
  t2 = fe_pow2k(t1, 10);
  t2 = fe_mul(t2, t1);
  Fe t3 = fe_pow2k(t2, 20);
  t2 = fe_mul(t3, t2);
  t2 = fe_pow2k(t2, 10);
  t1 = fe_mul(t2, t1);
  t2 = fe_pow2k(t1, 50);
  t2 = fe_mul(t2, t1);
  t3 = fe_pow2k(t2, 100);
  t2 = fe_mul(t3, t2);
  t2 = fe_pow2k(t2, 50);
  t1 = fe_mul(t2, t1);
  t1 = fe_pow2k(t1, 5);
  return fe_mul(t1, t0);
}

/* ---------- curve points: extended (ExtendedPoint) and affine (Point) ---------- */

typedef struct { Fe x, y, z, t; } Ext;
typedef struct { Fe x, y; } Aff;

static Ext ext_zero(void) {
  Ext p = {FE_ZERO, FE_ONE, FE_ONE, FE_ZERO};
  return p;
}

static int aff_is_zero(const Aff *a) { return fe_is_zero(&a->x) && fe_eq(&a->y, &FE_ONE); }

static Ext ext_from_aff(const Aff *a) {
  if (aff_is_zero(a)) return ext_zero();
  Ext p;
  p.x = a->x;
  p.y = a->y;
  p.z = FE_ONE;
  p.t = fe_mul(a->x, a->y);
  return p;
}

static Ext ext_double(const Ext *p) {
  Fe A = fe_sq(p->x);
  Fe B = fe_sq(p->y);
  Fe C = fe_add(fe_sq(p->z), fe_sq(p->z));
  Fe D = fe_neg(A); /* a = -1 */
  Fe x1y1 = fe_add(p->x, p->y);
  Fe E = fe_sub(fe_sub(fe_sq(x1y1), A), B);
  Fe G = fe_add(D, B);
  Fe F = fe_sub(G, C);
  Fe H = fe_sub(D, B);
  Ext r;
  r.x = fe_mul(E, F);
  r.y = fe_mul(G, H);
  r.t = fe_mul(E, H);
  r.z = fe_mul(F, G);
  return r;
}

static Ext ext_add(const Ext *p, const Ext *q) {
  Fe A = fe_mul(fe_sub(p->y, p->x), fe_add(q->y, q->x));
  Fe B = fe_mul(fe_add(p->y, p->x), fe_sub(q->y, q->x));
  Fe F = fe_sub(B, A);
  if (fe_is_zero(&F)) return ext_double(p);
  Fe C = fe_mul(fe_add(p->z, p->z), q->t);
  Fe D = fe_mul(fe_add(p->t, p->t), q->z);
  Fe E = fe_add(D, C);
  Fe G = fe_add(B, A);
  Fe H = fe_sub(D, C);
  Ext r;
  r.x = fe_mul(E, F);
  r.y = fe_mul(G, H);
  r.t = fe_mul(E, H);
  r.z = fe_mul(F, G);
  return r;
}

static Ext ext_negate(const Ext *p) {
  Ext r = *p;
  r.x = fe_neg(p->x);
  r.t = fe_neg(p->t);
  return r;
}

static int ext_equals(const Ext *p, const Ext *q) {
  Fe X1Z2 = fe_mul(p->x, q->z), X2Z1 = fe_mul(q->x, p->z);
  Fe Y1Z2 = fe_mul(p->y, q->z), Y2Z1 = fe_mul(q->y, p->z);
  return fe_eq(&X1Z2, &X2Z1) && fe_eq(&Y1Z2, &Y2Z1);
}

/* Montgomery batch inversion (invertBatch); zero inputs are skipped */
static void invert_batch(const Fe *zs, int n, Fe *out) {
  Fe acc = FE_ONE;
  for (int i = 0; i < n; i++) {
    if (fe_is_zero(&zs[i])) continue;
    out[i] = acc;
    acc = fe_mul(acc, zs[i]);
  }
  Fe inv = fe_invert(acc);
  for (int i = n - 1; i >= 0; i--) {
    if (fe_is_zero(&zs[i])) continue;
    out[i] = fe_mul(inv, out[i]);
    inv = fe_mul(inv, zs[i]);
  }
}

/* ExtendedPoint.toAffine for every point of a batch, then fromAffine (normalizeZ) */
static void ext_normalize(Ext *pts, int n) {
  Fe *zs = malloc((size_t)n * sizeof *zs);
  Fe *inv = malloc((size_t)n * sizeof *inv);
  if (!zs || !inv) die("out of memory");
  for (int i = 0; i < n; i++) zs[i] = pts[i].z;
  invert_batch(zs, n, inv);
  for (int i = 0; i < n; i++) {
    Aff a;
    a.x = fe_mul(pts[i].x, inv[i]);
    a.y = fe_mul(pts[i].y, inv[i]);
    Fe zz = fe_mul(pts[i].z, inv[i]);
    if (!fe_eq(&zz, &FE_ONE)) die("invZ was invalid");
    pts[i] = ext_from_aff(&a);
  }
  free(zs);
  free(inv);
}

/* ---------- base point window table (W = 8, 33 windows of 128 odd multiples) ---------- */

#define WIN_W 8
#define WIN_N 33
#define WIN_HALF 128
static Ext base_table[WIN_N * WIN_HALF];
static Aff BASE_AFF;
static Ext EXT_BASE;

/* Point.BASE.precomputes: table of (i+1) * 256^w * B, normalized to affine. */
static void precompute_base(void) {
  Ext p = ext_from_aff(&BASE_AFF);
  Ext *pts = base_table;
  for (int w = 0; w < WIN_N; w++) {
    Ext base = p;
    pts[w * WIN_HALF] = base;
    for (int i = 1; i < WIN_HALF; i++) {
      base = ext_add(&base, &p);
      pts[w * WIN_HALF + i] = base;
    }
    p = ext_double(&base);
  }
  ext_normalize(pts, WIN_N * WIN_HALF);
}

/* ExtendedPoint.wNAF with the base table: the scalar is 32 little-endian bytes. */
static Ext base_mul_ext(const u8 scalar[32]) {
  u8 n[WIN_N];
  memcpy(n, scalar, 32);
  n[32] = 0;
  Ext p = ext_zero(), f = ext_zero();
  for (int w = 0; w < WIN_N; w++) {
    int wbits = n[w];
    if (wbits > WIN_HALF) {
      wbits -= 256;
      for (int k = w + 1; k < WIN_N; k++) { /* n += 1 on the remaining value */
        n[k]++;
        if (n[k] != 0) break;
      }
    }
    int offset = w * WIN_HALF;
    if (wbits == 0) {
      Ext pr = base_table[offset];
      if (w % 2) pr = ext_negate(&pr);
      f = ext_add(&f, &pr);
    } else {
      int idx = (wbits < 0 ? -wbits : wbits) - 1;
      Ext cached = base_table[offset + idx];
      if (wbits < 0) cached = ext_negate(&cached);
      p = ext_add(&p, &cached);
    }
  }
  /* normalizeZ([p, f])[0] */
  Ext pair[2] = {p, f};
  ext_normalize(pair, 2);
  return pair[0];
}

/* Point.multiply on the base point: result as affine coordinates. */
static Aff base_mul(const u8 scalar[32]) {
  Ext e = base_mul_ext(scalar);
  Aff a = {e.x, e.y};
  return a;
}

/* ExtendedPoint.multiplyUnsafe: generic double-and-add, or the base table for B itself. */
static Ext mul_unsafe(const Ext *p, const u8 scalar[32]) {
  Big n;
  big_from_bytes(&n, scalar, 32);
  Ext zero = ext_zero();
  if (big_is_zero(&n)) return zero;
  Big one;
  big_zero(&one);
  one.w[0] = 1;
  if (ext_equals(p, &zero) || big_cmp(&n, &one) == 0) return *p;
  if (ext_equals(p, &EXT_BASE)) return base_mul_ext(scalar);
  Ext acc = zero, d = *p;
  int bits = big_bitlen(&n);
  for (int i = 0; i < bits; i++) {
    if (big_bit(&n, i)) acc = ext_add(&acc, &d);
    d = ext_double(&d);
  }
  return acc;
}

/* ---------- Point: encoding, decoding, Elligator / Ristretto ---------- */

static void point_encode(const Aff *p, u8 out[32]) {
  fe_to_bytes(p->y, out);
  out[31] |= (u8)(fe_is_negative(&p->x) ? 0x80 : 0);
}

/* uvRatio (noble): a square root of u/v, with the root choice made as in noble */
static int uv_ratio(Fe u, Fe v, Fe *value) {
  Fe v3 = fe_mul(fe_sq(v), v);
  Fe v7 = fe_mul(fe_sq(v3), v);
  Fe pw = fe_pow22523(fe_mul(u, v7));
  Fe x = fe_mul(fe_mul(u, v3), pw);
  Fe vx2 = fe_mul(v, fe_sq(x));
  Fe root2 = fe_mul(x, FE_SQRT_M1);
  Fe negu = fe_neg(u);
  Fe neg_u_sqrtm1 = fe_mul(negu, FE_SQRT_M1);
  int useRoot1 = fe_eq(&vx2, &u);
  int useRoot2 = fe_eq(&vx2, &negu);
  int noRoot = fe_eq(&vx2, &neg_u_sqrtm1);
  if (useRoot2 || noRoot) x = root2;
  if (fe_is_negative(&x)) x = fe_neg(x);
  *value = x;
  return useRoot1 || useRoot2;
}

static int inv_sqrt(Fe v, Fe *value) { return uv_ratio(FE_ONE, v, value); }

/* Point.fromHex: decompress a 32-byte encoding; strict rejects y >= p. */
static int point_from_hex(const u8 hex[32], int strict, Aff *out) {
  u8 normed[32];
  memcpy(normed, hex, 32);
  normed[31] &= 0x7f;
  Fe y = fe_from_bytes(normed);
  if (strict) {
    /* y >= p means the canonical reduction changed the input */
    u8 back[32];
    fe_to_bytes(y, back);
    if (memcmp(back, normed, 32) != 0) return 0;
  }
  Fe y2 = fe_sq(y);
  Fe u = fe_sub(y2, FE_ONE);
  Fe v = fe_add(fe_mul(FE_D, y2), FE_ONE);
  Fe x;
  if (!uv_ratio(u, v, &x)) return 0;
  int isXOdd = fe_is_negative(&x);
  int isLastByteOdd = (hex[31] & 0x80) != 0;
  if (isLastByteOdd != isXOdd) x = fe_neg(x);
  out->x = x;
  out->y = y;
  return 1;
}

/* RistrettoPoint.calcElligatorRistrettoMap */
static Ext elligator_map(Fe r0) {
  Fe r = fe_mul(FE_SQRT_M1, fe_sq(r0));
  Fe Ns = fe_mul(fe_add(r, FE_ONE), FE_ONE_MINUS_D_SQ);
  Fe c = fe_neg(FE_ONE);
  Fe D = fe_mul(fe_sub(c, fe_mul(FE_D, r)), fe_add(r, FE_D));
  Fe s;
  int Ns_D_is_sq = uv_ratio(Ns, D, &s);
  Fe s_ = fe_mul(s, r0);
  if (!fe_is_negative(&s_)) s_ = fe_neg(s_);
  if (!Ns_D_is_sq) s = s_;
  if (!Ns_D_is_sq) c = r;
  Fe Nt = fe_sub(fe_mul(fe_mul(c, fe_sub(r, FE_ONE)), FE_D_MINUS_ONE_SQ), D);
  Fe s2 = fe_sq(s);
  Fe W0 = fe_mul(fe_add(s, s), D);
  Fe W1 = fe_mul(Nt, FE_SQRT_AD_MINUS_ONE);
  Fe W2 = fe_sub(FE_ONE, s2);
  Fe W3 = fe_add(FE_ONE, s2);
  Ext p;
  p.x = fe_mul(W0, W3);
  p.y = fe_mul(W2, W1);
  p.z = fe_mul(W1, W3);
  p.t = fe_mul(W0, W2);
  return p;
}

/* bytes255ToNumberLE: clear the top bit, then reduce modulo p */
static Fe bytes255_to_fe(const u8 *b) {
  u8 m[32];
  memcpy(m, b, 32);
  m[31] &= 0x7f;
  return fe_from_bytes(m);
}

static Ext ristretto_hash_to_curve(const u8 hex[64]) {
  Ext R1 = elligator_map(bytes255_to_fe(hex));
  Ext R2 = elligator_map(bytes255_to_fe(hex + 32));
  return ext_add(&R1, &R2);
}

/* RistrettoPoint.fromHex; returns 0 for an invalid encoding. */
static int ristretto_from_hex(const u8 hex[32], Ext *out) {
  Fe s = bytes255_to_fe(hex);
  u8 back[32];
  fe_to_bytes(s, back);
  if (memcmp(back, hex, 32) != 0 || fe_is_negative(&s)) return 0;
  Fe a_s2 = fe_neg(fe_sq(s)); /* a * s^2 with a = -1 */
  Fe u1 = fe_add(FE_ONE, a_s2);
  Fe u2 = fe_sub(FE_ONE, a_s2);
  Fe u1_2 = fe_sq(u1);
  Fe u2_2 = fe_sq(u2);
  Fe v = fe_sub(fe_neg(fe_mul(FE_D, u1_2)), u2_2);
  Fe I;
  int isValid = inv_sqrt(fe_mul(v, u2_2), &I);
  Fe Dx = fe_mul(I, u2);
  Fe Dy = fe_mul(fe_mul(I, Dx), v);
  Fe x = fe_mul(fe_add(s, s), Dx);
  if (fe_is_negative(&x)) x = fe_neg(x);
  Fe y = fe_mul(u1, Dy);
  Fe t = fe_mul(x, y);
  if (!isValid || fe_is_negative(&t) || fe_is_zero(&y)) return 0;
  out->x = x;
  out->y = y;
  out->z = FE_ONE;
  out->t = t;
  return 1;
}

/* RistrettoPoint.toRawBytes */
static void ristretto_encode(const Ext *ep, u8 out[32]) {
  Fe x = ep->x, y = ep->y, z = ep->z, t = ep->t;
  Fe u1 = fe_mul(fe_add(z, y), fe_sub(z, y));
  Fe u2 = fe_mul(x, y);
  Fe u2sq = fe_sq(u2);
  Fe invsqrt;
  inv_sqrt(fe_mul(u1, u2sq), &invsqrt);
  Fe D1 = fe_mul(invsqrt, u1);
  Fe D2 = fe_mul(invsqrt, u2);
  Fe zInv = fe_mul(fe_mul(D1, D2), t);
  Fe D = D2;
  Fe tz = fe_mul(t, zInv);
  if (fe_is_negative(&tz)) {
    Fe nx = fe_mul(y, FE_SQRT_M1);
    Fe ny = fe_mul(x, FE_SQRT_M1);
    x = nx;
    y = ny;
    D = fe_mul(D1, FE_INVSQRT_A_MINUS_D);
  } else {
    D = D2;
  }
  Fe xz = fe_mul(x, zInv);
  if (fe_is_negative(&xz)) y = fe_neg(y);
  Fe s = fe_mul(fe_sub(z, y), D);
  if (fe_is_negative(&s)) s = fe_neg(s);
  fe_to_bytes(s, out);
}

/* ---------- constants, decoded once from the decimal values in the bundle ---------- */

static void init_constants(void) {
  big_from_dec(&BIG_L, "7237005577332262213973186563042994240857116359379907606001950938285454250989");
  FE_D = fe_from_dec("37095705934669439343138083508754565189542113879843219016388785533085940283555");
  FE_SQRT_M1 = fe_from_dec("19681161376707505956807079304988542015446066515923890162744021073123829784752");
  FE_SQRT_AD_MINUS_ONE = fe_from_dec("25063068953384623474111414158702152701244531502492656460079210482610430750235");
  FE_INVSQRT_A_MINUS_D = fe_from_dec("54469307008909316920995813868745141605393597292927456921205312896311721017578");
  FE_ONE_MINUS_D_SQ = fe_from_dec("1159843021668779879193775521855586647937357759715417654439879720876111806838");
  FE_D_MINUS_ONE_SQ = fe_from_dec("40440834346308536858101042469323190826248399146238708352240133220865137265952");
  FE_GX = fe_from_dec("15112221349535400772501151409588531511454012693041857206046113283949847762202");
  FE_GY = fe_from_dec("46316835694926478169428394003475163141307993866256225615783033603165251855960");
  BASE_AFF.x = FE_GX;
  BASE_AFF.y = FE_GY;
  EXT_BASE = ext_from_aff(&BASE_AFF);
}

/* ---------- byte helpers, checksum, deterministic randomness ---------- */

static u64 checksum = 1469598103934665603ULL;

static void mix_bytes(const u8 *p, size_t n) {
  for (size_t i = 0; i < n; i++) checksum = (checksum ^ p[i]) * 1099511628211ULL;
}

static void mix_fe(const Fe *f) {
  u8 b[32];
  fe_to_bytes(*f, b);
  mix_bytes(b, 32);
}

static void mix_int(int v) {
  u8 b = (u8)v;
  mix_bytes(&b, 1);
}

static int hexval(char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  die("bad hex digit");
  return 0;
}

/* to<N>Bytes: left-pad a hex string with zeros to n bytes and decode it big-endian */
static void hex_to_bytes_padded(const char *hex, u8 *out, size_t n) {
  size_t len = strlen(hex);
  if (len > 2 * n) die("hex string too long");
  memset(out, 0, n);
  for (size_t i = 0; i < len; i++) {
    size_t pos = 2 * n - len + i;
    u8 v = (u8)hexval(hex[i]);
    if (pos % 2 == 0)
      out[pos / 2] = (u8)(v << 4);
    else
      out[pos / 2] |= v;
  }
}

static u64 rng_state = 0x2545F4914F6CDD1DULL;

static u64 rng_next(void) {
  rng_state ^= rng_state >> 12;
  rng_state ^= rng_state << 25;
  rng_state ^= rng_state >> 27;
  return rng_state * 0x2545F4914F6CDD1DULL;
}

/* stands in for ed.utils.randomPrivateKey (the JS side is seeded through deterministicRandom) */
static void rng_bytes(u8 *out, size_t n) {
  for (size_t i = 0; i < n; i++) out[i] = (u8)(rng_next() >> 24);
}

static void adjust_bytes25519(u8 b[32]) {
  b[0] &= 248;
  b[31] &= 127;
  b[31] |= 64;
}

/* ---------- keys, signatures, verification (noble getKeyFromHash / sign / verify) ---------- */

typedef struct {
  u8 head[32];
  u8 prefix[32];
  u8 scalar[32];
  u8 pub[32];
} ExtKey;

static void ext_key_from_priv(const u8 priv[32], ExtKey *k) {
  u8 h[64];
  sha512_cat(h, priv, 32, NULL, 0, NULL, 0);
  memcpy(k->head, h, 32);
  adjust_bytes25519(k->head);
  memcpy(k->prefix, h + 32, 32);
  Big s = modl_le(k->head, 32);
  big_to_bytes(&s, k->scalar, 32);
  Aff p = base_mul(k->scalar);
  point_encode(&p, k->pub);
}

static void get_public_key(const u8 priv[32], u8 pub[32]) {
  ExtKey k;
  ext_key_from_priv(priv, &k);
  memcpy(pub, k.pub, 32);
}

static void ed_sign(const u8 *msg, size_t mlen, const u8 priv[32], u8 sig[64]) {
  ExtKey k;
  ext_key_from_priv(priv, &k);
  u8 h[64];
  sha512_cat(h, k.prefix, 32, msg, mlen, NULL, 0);
  Big r = modl_le(h, 64);
  u8 rb[32];
  big_to_bytes(&r, rb, 32);
  Aff R = base_mul(rb);
  u8 Renc[32];
  point_encode(&R, Renc);
  sha512_cat(h, Renc, 32, k.pub, 32, msg, mlen);
  Big kk = modl_le(h, 64);
  Big sc, t, s;
  big_from_bytes(&sc, k.scalar, 32);
  big_mul(&t, &kk, &sc);
  big_add(&t, &t, &r);
  big_mod(&s, &t, &BIG_L);
  memcpy(sig, Renc, 32);
  big_to_bytes(&s, sig + 32, 32);
}

/* must_point: Point.fromHex, which throws in JS; here an invalid input stops the benchmark */
static Aff must_point(const u8 *hex, int strict) {
  Aff a;
  if (!point_from_hex(hex, strict, &a)) die("Point.fromHex: invalid encoding");
  return a;
}

/* verify with a decoded signature (R, s) and public key, as the Signature/Point instance path */
static int ed_verify_parsed(const Aff *R, const Big *s, const Aff *pub, const u8 *msg, size_t mlen) {
  if (big_cmp(s, &BIG_L) >= 0) die("Signature.assertValidity: s >= l");
  u8 sb[32];
  big_to_bytes(s, sb, 32);
  Ext SB = mul_unsafe(&EXT_BASE, sb);
  u8 Renc[32], Penc[32], h[64];
  point_encode(R, Renc);
  point_encode(pub, Penc);
  sha512_cat(h, Renc, 32, Penc, 32, msg, mlen);
  Big kb = modl_le(h, 64);
  u8 kbytes[32];
  big_to_bytes(&kb, kbytes, 32);
  Ext P = ext_from_aff(pub);
  Ext kA = mul_unsafe(&P, kbytes);
  Ext RA = ext_from_aff(R);
  Ext RkA = ext_add(&RA, &kA);
  Ext negSB = ext_negate(&SB);
  Ext diff = ext_add(&RkA, &negSB);
  u8 eight[32] = {8};
  Ext res = mul_unsafe(&diff, eight);
  Ext zero = ext_zero();
  return ext_equals(&res, &zero);
}

static int ed_verify(const u8 sig[64], const u8 *msg, size_t mlen, const u8 pub[32]) {
  Aff P = must_point(pub, 0);
  Aff R = must_point(sig, 0);
  Big s;
  big_from_bytes(&s, sig + 32, 32);
  return ed_verify_parsed(&R, &s, &P, msg, mlen);
}

/* ---------- X25519 (noble montgomeryLadder, scalarMult, scalarMultBase) ---------- */

static void fe_cswap(int swap, Fe *a, Fe *b) {
  u64 mask = (u64)0 - (u64)(swap & 1);
  for (int i = 0; i < 4; i++) {
    u64 t = mask & (a->v[i] ^ b->v[i]);
    a->v[i] ^= t;
    b->v[i] ^= t;
  }
}

/* returns 0 when the result is the zero point (JS throws there) */
static int x25519(const u8 scalar_in[32], const u8 u_in[32], u8 out[32]) {
  u8 k[32], uu[32];
  memcpy(k, scalar_in, 32);
  adjust_bytes25519(k);
  memcpy(uu, u_in, 32);
  uu[31] &= 127;
  Fe u = fe_from_bytes(uu);
  Fe A24 = fe_from_u64(121665);
  Fe x1 = u, x2 = FE_ONE, z2 = FE_ZERO, x3 = u, z3 = FE_ONE;
  int swap = 0;
  for (int t = 254; t >= 0; t--) {
    int kt = (k[t / 8] >> (t % 8)) & 1;
    swap ^= kt;
    fe_cswap(swap, &x2, &x3);
    fe_cswap(swap, &z2, &z3);
    swap = kt;
    Fe A = fe_add(x2, z2);
    Fe AA = fe_sq(A);
    Fe B = fe_sub(x2, z2);
    Fe BB = fe_sq(B);
    Fe E = fe_sub(AA, BB);
    Fe C = fe_add(x3, z3);
    Fe D = fe_sub(x3, z3);
    Fe DA = fe_mul(D, A);
    Fe CB = fe_mul(C, B);
    Fe dacb = fe_add(DA, CB);
    Fe da_cb = fe_sub(DA, CB);
    x3 = fe_sq(dacb);
    z3 = fe_mul(x1, fe_sq(da_cb));
    x2 = fe_mul(AA, BB);
    z2 = fe_mul(E, fe_add(AA, fe_mul(A24, E)));
  }
  fe_cswap(swap, &x2, &x3);
  fe_cswap(swap, &z2, &z3);
  /* z2^(p-2) = (z2^(2^252-3))^8 * z2^3 */
  Fe pw = fe_pow22523(z2);
  Fe b2 = fe_mul(fe_sq(z2), z2);
  Fe xp2 = fe_mul(fe_pow2k(pw, 3), b2);
  Fe res = fe_mul(x2, xp2);
  fe_to_bytes(res, out);
  return !fe_is_zero(&res);
}

static void x25519_scalar_mult_base(const u8 scalar[32], u8 out[32]) {
  static const u8 base_u[32] = {9};
  if (!x25519(scalar, base_u, out)) die("Invalid private or public key received");
}

/* getSharedSecret: the private key's head is the scalar, the public key's Montgomery u the input */
static void ed_shared_secret(const u8 priv[32], const u8 pub[32], u8 out[32]) {
  ExtKey k;
  ext_key_from_priv(priv, &k);
  Aff P = must_point(pub, 1);
  Fe one_plus = fe_add(FE_ONE, P.y);
  Fe one_minus = fe_sub(FE_ONE, P.y);
  Fe uu = fe_mul(one_plus, fe_invert(one_minus));
  u8 u[32];
  fe_to_bytes(uu, u);
  if (!x25519(k.head, u, out)) die("Invalid private or public key received");
}

/* ---------- one benchmark iteration (runNobleBenchmark) ---------- */

static void run_iteration(void) {
  u8 priv1bit[32], priv[32], msg[32], pub[32], sig[64], rpriv[32], out[32];
  u8 hash[64];
  u8 enc2[32];
  u8 scalar_aead[32], priv12345[32];
  hex_to_bytes_padded("2", priv1bit, 32);
  hex_to_bytes_padded("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60", priv, 32);
  hex_to_bytes_padded("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", msg, 32);
  hex_to_bytes_padded("6a493210f7499cd17fecb510ae0cea23a110e8d5b901f8acadd3095c73a3b919", enc2, 32);
  hex_to_bytes_padded("aeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef", scalar_aead, 32);
  memset(priv12345, 0, 32);
  priv12345[29] = 0x01;
  priv12345[30] = 0x23;
  priv12345[31] = 0x45;
  for (int i = 0; i < 64; i++) hash[i] = (i % 4 == 0) ? 0xde : (i % 4 == 1) ? 0xad : (i % 4 == 2) ? 0xbe : 0xef;

  precompute_base();

  for (int i = 0; i < 30; i++) get_public_key(priv1bit, pub);
  for (int i = 0; i < 30; i++) {
    rng_bytes(rpriv, 32);
    get_public_key(rpriv, pub);
  }
  mix_bytes(pub, 32);
  for (int i = 0; i < 20; i++) ed_sign(msg, 32, priv, sig);
  mix_bytes(sig, 64);
  for (int i = 0; i < 4; i++) mix_int(ed_verify(sig, msg, 32, pub));

  /* Signature.fromHex and Point.fromHex: decoded once, verified five times */
  Aff sigR = must_point(sig, 0);
  Big sigS;
  big_from_bytes(&sigS, sig + 32, 32);
  Aff pubPt = must_point(pub, 1);
  for (int i = 0; i < 5; i++) mix_int(ed_verify_parsed(&sigR, &sigS, &pubPt, msg, 32));

  for (int i = 0; i < 65; i++) {
    Aff a = must_point(pub, 1);
    mix_fe(&a.x);
  }

  for (int i = 0; i < 30; i++) {
    Ext h = ristretto_hash_to_curve(hash);
    mix_fe(&h.x);
    mix_fe(&h.t);
  }

  for (int i = 0; i < 30; i++) {
    Ext p;
    if (!ristretto_from_hex(enc2, &p)) die("RistrettoPoint.fromHex: invalid encoding");
    u8 back[32];
    ristretto_encode(&p, back);
    mix_bytes(back, 32);
  }

  for (int i = 0; i < 6; i++) {
    x25519_scalar_mult_base(scalar_aead, out);
    mix_bytes(out, 32);
  }

  for (int i = 0; i < 5; i++) {
    ed_shared_secret(priv12345, scalar_aead, out);
    mix_bytes(out, 32);
  }

  /* sync API: same operations with the synchronous sha512 */
  for (int i = 0; i < 30; i++) {
    rng_bytes(rpriv, 32);
    get_public_key(rpriv, out);
    mix_bytes(out, 32);
  }
  for (int i = 0; i < 20; i++) ed_sign(msg, 32, priv, sig);
  mix_bytes(sig, 64);
  for (int i = 0; i < 4; i++) mix_int(ed_verify(sig, msg, 32, pub));
}

/* untimed checks: RFC 8032 test 1 public key, a sign/verify round trip, a Ristretto round trip */
static void validate(void) {
  u8 priv[32], want[32], pub[32], msg[32], sig[64], enc[32], back[32];
  hex_to_bytes_padded("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60", priv, 32);
  hex_to_bytes_padded("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a", want, 32);
  hex_to_bytes_padded("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", msg, 32);
  hex_to_bytes_padded("6a493210f7499cd17fecb510ae0cea23a110e8d5b901f8acadd3095c73a3b919", enc, 32);
  precompute_base();
  get_public_key(priv, pub);
  if (memcmp(pub, want, 32) != 0) die("validation: public key does not match RFC 8032 test 1");
  ed_sign(msg, 32, priv, sig);
  if (!ed_verify(sig, msg, 32, pub)) die("validation: signature does not verify");
  Ext p;
  if (!ristretto_from_hex(enc, &p)) die("validation: Ristretto decode failed");
  ristretto_encode(&p, back);
  if (memcmp(back, enc, 32) != 0) die("validation: Ristretto round trip mismatch");
}

static double now_ms(void) {
  struct timespec ts;
  timespec_get(&ts, TIME_UTC);
  return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 30;
  init_constants();
  double t0 = now_ms();
  for (int i = 0; i < iterations; i++) run_iteration();
  double t1 = now_ms();
  validate();
  printf("bigint-noble-ed25519 %d iterations %.3f ms\n", iterations, t1 - t0);
  printf("checksum %016llx\n", (unsigned long long)checksum);
  return 0;
}
