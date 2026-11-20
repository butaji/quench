// jetstream-row: bomb-workers
// jetstream-files: worker/bomb.js
// iterations: 80
// jetstream-driver: class=Async preload=rayTrace3D=worker/bomb-subtests/3d-raytrace.js,accessNbody=worker/bomb-subtests/access-nbody.js,morph3D=worker/bomb-subtests/3d-morph.js,cube3D=worker/bomb-subtests/3d-cube.js,accessFunnkuch=worker/bomb-subtests/access-fannkuch.js,accessBinaryTrees=worker/bomb-subtests/access-binary-trees.js,accessNsieve=worker/bomb-subtests/access-nsieve.js,bitopsBitwiseAnd=worker/bomb-subtests/bitops-bitwise-and.js,bitopsNsieveBits=worker/bomb-subtests/bitops-nsieve-bits.js,controlflowRecursive=worker/bomb-subtests/controlflow-recursive.js,bitops3BitBitsInByte=worker/bomb-subtests/bitops-3bit-bits-in-byte.js,botopsBitsInByte=worker/bomb-subtests/bitops-bits-in-byte.js,cryptoAES=worker/bomb-subtests/crypto-aes.js,cryptoMD5=worker/bomb-subtests/crypto-md5.js,cryptoSHA1=worker/bomb-subtests/crypto-sha1.js,dateFormatTofte=worker/bomb-subtests/date-format-tofte.js,dateFormatXparb=worker/bomb-subtests/date-format-xparb.js,mathCordic=worker/bomb-subtests/math-cordic.js,mathPartialSums=worker/bomb-subtests/math-partial-sums.js,mathSpectralNorm=worker/bomb-subtests/math-spectral-norm.js,stringBase64=worker/bomb-subtests/string-base64.js,stringFasta=worker/bomb-subtests/string-fasta.js,stringValidateInput=worker/bomb-subtests/string-validate-input.js,stringTagcloud=worker/bomb-subtests/string-tagcloud.js,stringUnpackCode=worker/bomb-subtests/string-unpack-code.js,regexpDNA=worker/bomb-subtests/regexp-dna.js worker=true
/* C analogue of JetStream's bomb-workers row (worker/bomb.js). The JS row starts one Worker per
 * subtest, and each worker runs its subtest script once and posts "done"; an iteration is all 26
 * subtests. This analogue runs the same 26 subtests in the same order, each once per iteration,
 * on one thread. Every subtest is a C port of the JS file, with the same inputs, the same
 * algorithm and operation order, and the subtest's own validation (an exact result, a checksum, a
 * string length, or a round trip). The literal data the JS engine evaluates on each run (the DNA
 * sequence, the packed libraries, the tag JSON, the plain texts) is read from the files at setup
 * and decoded on every iteration. The JS source is not parsed by C (no JS parser here), and the
 * JS-only harness (the Worker and postMessage protocol) is replaced by the direct calls.
 * Math.sin, Math.cos, Math.pow and Math.log use V8's fdlibm routines (v8-math-analogue.h), and
 * numbers print as ECMAScript prints them, because the checks depend on exact doubles and on
 * the lengths of printed numbers. Math.random is a fixed-seed generator: the random subtests
 * check round trips and lengths that do not depend on the particular values.
 * Usage: bomb-workers [iterations] [jetstream-root]; run from the repo root. */
#define _DEFAULT_SOURCE
#include <ctype.h>
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "v8-math-analogue.h"

#define JS_PI 3.141592653589793

/* ---- growable byte buffer: the JS string built by +=, and the output of join() ---- */
typedef struct { char *p; size_t n, cap; } Str;
static void str_grow(Str *s, size_t extra) {
  if (s->n + extra + 1 <= s->cap) return;
  size_t cap = (s->n + extra + 1) * 2;
  s->p = realloc(s->p, cap);
  if (!s->p) { fprintf(stderr, "out of memory\n"); exit(2); }
  s->cap = cap;
}
static void str_add(Str *s, const char *p, size_t n) {
  str_grow(s, n);
  memcpy(s->p + s->n, p, n);
  s->n += n;
  s->p[s->n] = 0;
}
static void str_cat(Str *s, const char *c) { str_add(s, c, strlen(c)); }
static void str_char(Str *s, char c) { str_add(s, &c, 1); }
static void str_int(Str *s, long long v) { char t[32]; int k = snprintf(t, sizeof t, "%lld", v); str_add(s, t, (size_t)k); }
static void str_num(Str *s, double v) { char t[40]; int k = v8m_number_to_string(v, t); str_add(s, t, (size_t)k); }
static void str_double(Str *s) {          /* s += s; grow first, then copy from the (possibly moved) buffer */
  str_grow(s, s->n);
  memcpy(s->p + s->n, s->p, s->n);
  s->n *= 2;
  s->p[s->n] = 0;
}
static void str_free(Str *s) { free(s->p); s->p = NULL; s->n = s->cap = 0; }

/* ---- files and JS string literals ---- */
static char *read_file(const char *path, size_t *len) {
  FILE *f = fopen(path, "rb");
  if (!f) { fprintf(stderr, "cannot open %s\n", path); exit(2); }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *buf = malloc((size_t)n + 1);
  if (fread(buf, 1, (size_t)n, f) != (size_t)n) { fprintf(stderr, "short read %s\n", path); exit(2); }
  buf[n] = 0;
  fclose(f);
  *len = (size_t)n;
  return buf;
}
static const char *find_after(const char *hay, const char *needle) {
  const char *p = strstr(hay, needle);
  if (!p) { fprintf(stderr, "missing %s\n", needle); exit(2); }
  return p + strlen(needle);
}
/* Decodes the JS string literal whose body starts at p and ends at the first unescaped `quote`.
 * Escapes and line continuations are decoded as the engine does. Code points above 0xff do not
 * occur in these sources, so they are rejected. Sets *end to the closing quote. */
static void js_literal(const char *p, char quote, Str *out, const char **end) {
  str_grow(out, 1);
  out->n = 0;
  out->p[0] = 0;
  while (*p != quote) {
    if (!*p) { fprintf(stderr, "unterminated string literal\n"); exit(2); }
    if (*p != '\\') { str_char(out, *p++); continue; }
    p++;
    if (*p == '\r') { p++; if (*p == '\n') p++; continue; }
    if (*p == '\n') { p++; continue; }
    char c = *p;
    switch (c) {
      case 'n': str_char(out, '\n'); p++; break;
      case 't': str_char(out, '\t'); p++; break;
      case 'r': str_char(out, '\r'); p++; break;
      case 'b': str_char(out, '\b'); p++; break;
      case 'f': str_char(out, '\f'); p++; break;
      case 'v': str_char(out, '\v'); p++; break;
      case '0': str_char(out, 0); p++; break;
      case 'x': case 'u': {
        int digits = c == 'x' ? 2 : 4;
        unsigned cp = 0;
        p++;
        for (int k = 0; k < digits; k++, p++) {
          int d = isdigit((unsigned char)*p) ? *p - '0' : (tolower((unsigned char)*p) >= 'a' && tolower((unsigned char)*p) <= 'f') ? tolower((unsigned char)*p) - 'a' + 10 : -1;
          if (d < 0) { fprintf(stderr, "bad escape in literal\n"); exit(2); }
          cp = cp * 16 + (unsigned)d;
        }
        if (cp > 0xff) { fprintf(stderr, "code point above 0xff in literal\n"); exit(2); }
        str_char(out, (char)cp);
        break;
      }
      default: str_char(out, c); p++; break;  /* identity escapes: \' \" \\ and so on */
    }
  }
  if (end) *end = p;
}

/* ---- JS numeric helpers ---- */
static int32_t to_int32(double d) { return (int32_t)(uint32_t)(int64_t)d; }    /* integral d, |d| < 2^63 */
static uint32_t to_uint32(double x) {                                         /* ECMAScript ToUint32 */
  if (!isfinite(x)) return 0;
  double m = fmod(trunc(x), 4294967296.0);
  if (m < 0) m += 4294967296.0;
  return (uint32_t)m;
}
static double js_round(double x) {                                            /* Math.round, x >= 0 */
  double f = floor(x);
  return (x - f >= 0.5) ? f + 1 : f;
}

/* ---- deterministic stand-in for Math.random (xorshift64*), kept across iterations ---- */
static uint64_t rng_state = 0x9e3779b97f4a7c15ULL;
static double rng_next(void) {
  rng_state ^= rng_state >> 12;
  rng_state ^= rng_state << 25;
  rng_state ^= rng_state >> 27;
  return (double)((rng_state * 0x2545F4914F6CDD1DULL) >> 11) / 9007199254740992.0;
}

static int64_t epoch_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_REALTIME, &ts);
  return (int64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
}
static double now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec * 1000.0 + ts.tv_nsec / 1e6;
}

/* Failure reporting: each subtest returns the number of failed checks. */
static int bomb_fail(const char *name, const char *what, double got, double expected) {
  fprintf(stderr, "%s: bad result: expected %.17g but got %.17g (%s)\n", name, expected, got, what);
  return 1;
}

/* Data read at setup, as the worker's preloaded sources (JetStream.preload). */
typedef struct {
  char *dna, *unpack, *tagcloud, *md5, *sha1, *aes, *fasta;
  size_t dna_n, unpack_n, tagcloud_n, md5_n, sha1_n, aes_n, fasta_n;
} Sources;
static Sources S;
static char *g_root;

/* ---- controlflow-recursive ---- */
static int64_t ack(int64_t m, int64_t n) {
  if (m == 0) return n + 1;
  if (n == 0) return ack(m - 1, 1);
  return ack(m - 1, ack(m, n - 1));
}
static double fib(double n) {
  if (n < 2) return 1;
  return fib(n - 2) + fib(n - 1);
}
static double tak(double x, double y, double z) {
  if (y >= x) return z;
  return tak(tak(x - 1, y, z), tak(y - 1, z, x), tak(z - 1, x, y));
}
static int run_controlflow_recursive(void) {
  double result = 0;
  for (int i = 3; i <= 5; i++) {
    result += (double)ack(3, i);
    result += fib(17.0 + i);
    result += tak(3 * i + 3, 2 * i + 2, i + 1);
  }
  if (result != 57775) return bomb_fail("controlflow-recursive", "checksum", result, 57775);
  return 0;
}

/* ---- bitops-3bit-bits-in-byte and bitops-bits-in-byte ---- */
static int fast3bitlookup(int b) {
  int bi3b = 0xE994;
  int c = 3 & (bi3b >> ((b << 1) & 14));
  c += 3 & (bi3b >> ((b >> 2) & 14));
  c += 3 & (bi3b >> ((b >> 5) & 6));
  return c;
}
static int bitsinbyte(int b) {
  int m = 1, c = 0;
  while (m < 0x100) {
    if (b & m) c++;
    m <<= 1;
  }
  return c;
}
static int run_bitops_3bit(void) {
  double sum = 0;
  for (int x = 0; x < 500; x++)
    for (int y = 0; y < 256; y++) sum += fast3bitlookup(y);
  if (sum != 512000) return bomb_fail("bitops-3bit-bits-in-byte", "sum", sum, 512000);
  return 0;
}
static int run_bitops_bits_in_byte(void) {
  double sum = 0;
  for (int x = 0; x < 350; x++)
    for (int y = 0; y < 256; y++) sum += bitsinbyte(y);
  if (sum != 358400) return bomb_fail("bitops-bits-in-byte", "sum", sum, 358400);
  return 0;
}

/* ---- bitops-bitwise-and: the start value is ToInt32(2^32) = 0 ---- */
static int run_bitops_bitwise_and(void) {
  int32_t v = to_int32(4294967296.0);
  for (int i = 0; i < 600000; i++) v = v & i;
  if (v != 0) return bomb_fail("bitops-bitwise-and", "result", v, 0);
  return 0;
}

/* ---- bitops-nsieve-bits: elements are JS numbers, untouched ones 0xffffffff, touched ones int32 ---- */
static double nsieve_bits[5000];
static void nsieve_bits_primes(double *isPrime, int n) {
  int m = 10000 << n;
  int size = (m + 31) >> 5;
  for (int i = 0; i < size; i++) isPrime[i] = 4294967295.0;
  int count = 0;
  for (int i = 2; i < m; i++)
    if (to_int32(isPrime[i >> 5]) & (int32_t)(1u << (i & 31))) {
      for (int j = i + i; j < m; j += i)
        isPrime[j >> 5] = (double)(to_int32(isPrime[j >> 5]) & ~(int32_t)(1u << (j & 31)));
      count++;
    }
  (void)count;
}
static int run_bitops_nsieve_bits(void) {
  nsieve_bits_primes(nsieve_bits, 4);  /* new Array((10000<<4)+31>>5) = 5000 elements */
  double sum = 0;
  for (int i = 0; i < 5000; ++i) sum += nsieve_bits[i];
  if (sum != -1286749544853.0) return bomb_fail("bitops-nsieve-bits", "sum", sum, -1286749544853.0);
  return 0;
}

/* ---- access-nsieve ---- */
static int run_access_nsieve(void) {
  double sum = 0;
  for (int i = 1; i <= 3; i++) {
    int m = (1 << i) * 10000;
    bool *isPrime = malloc((size_t)m + 1);
    int count = 0;
    for (int k = 2; k <= m; k++) isPrime[k] = true;
    for (int k = 2; k <= m; k++) {
      if (isPrime[k]) {
        for (int j = k + k; j <= m; j += k) isPrime[j] = false;
        count++;
      }
    }
    sum += count;
    free(isPrime);
  }
  if (sum != 14302) return bomb_fail("access-nsieve", "count", sum, 14302);
  return 0;
}

/* ---- access-binary-trees ---- */
typedef struct Tree { struct Tree *left, *right; int item; } Tree;
static Tree *bottom_up_tree(int item, int depth) {
  Tree *t = malloc(sizeof *t);
  if (depth > 0) {
    t->left = bottom_up_tree(2 * item - 1, depth - 1);
    t->right = bottom_up_tree(2 * item, depth - 1);
  } else {
    t->left = t->right = NULL;
  }
  t->item = item;
  return t;
}
static long long item_check(const Tree *t) {
  if (t->left == NULL) return t->item;
  return t->item + item_check(t->left) - item_check(t->right);
}
static void free_tree(Tree *t) {
  if (!t) return;
  free_tree(t->left);
  free_tree(t->right);
  free(t);
}
static int run_access_binary_trees(void) {
  long long ret = 0;
  for (int n = 4; n <= 7; n += 1) {
    int minDepth = 4;
    int maxDepth = minDepth + 2 > n ? minDepth + 2 : n;
    int stretchDepth = maxDepth + 1;
    Tree *t = bottom_up_tree(0, stretchDepth);
    long long check = item_check(t);
    free_tree(t);
    Tree *longLived = bottom_up_tree(0, maxDepth);
    for (int depth = minDepth; depth <= maxDepth; depth += 2) {
      int iterations = 1 << (maxDepth - depth + minDepth);
      check = 0;
      for (int i = 1; i <= iterations; i++) {
        Tree *a = bottom_up_tree(i, depth);
        check += item_check(a);
        free_tree(a);
        Tree *b = bottom_up_tree(-i, depth);
        check += item_check(b);
        free_tree(b);
      }
    }
    volatile long long sink = check;
    (void)sink;
    ret += item_check(longLived);
    free_tree(longLived);
  }
  if (ret != -4) return bomb_fail("access-binary-trees", "checksum", (double)ret, -4);
  return 0;
}

/* ---- access-fannkuch (n = 8) ---- */
static int fannkuch(int n) {
  int check = 0;
  int *perm = malloc(sizeof(int) * (size_t)n), *perm1 = malloc(sizeof(int) * (size_t)n);
  int *count = malloc(sizeof(int) * (size_t)n), *maxPerm = malloc(sizeof(int) * (size_t)n);
  int maxFlipsCount = 0;
  int m = n - 1;
  char s[64];
  for (int i = 0; i < n; i++) perm1[i] = i;
  int r = n;
  for (;;) {
    if (check < 30) {
      int sl = 0;
      for (int i = 0; i < n; i++) sl += sprintf(s + sl, "%d", perm1[i] + 1);
      check++;
    }
    while (r != 1) { count[r - 1] = r; r--; }
    if (!(perm1[0] == 0 || perm1[m] == m)) {
      for (int i = 0; i < n; i++) perm[i] = perm1[i];
      int flipsCount = 0;
      int k;
      while (!((k = perm[0]) == 0)) {
        int k2 = (k + 1) >> 1;
        for (int i = 0; i < k2; i++) {
          int temp = perm[i]; perm[i] = perm[k - i]; perm[k - i] = temp;
        }
        flipsCount++;
      }
      if (flipsCount > maxFlipsCount) {
        maxFlipsCount = flipsCount;
        for (int i = 0; i < n; i++) maxPerm[i] = perm1[i];
      }
    }
    for (;;) {
      if (r == n) {
        free(perm); free(perm1); free(count); free(maxPerm);
        return maxFlipsCount;
      }
      int perm0 = perm1[0];
      int i = 0;
      while (i < r) {
        int j = i + 1;
        perm1[i] = perm1[j];
        i = j;
      }
      perm1[r] = perm0;
      count[r] = count[r] - 1;
      if (count[r] > 0) break;
      r++;
    }
  }
}
static int run_access_fannkuch(void) {
  int ret = fannkuch(8);
  if (ret != 22) return bomb_fail("access-fannkuch", "flips", ret, 22);
  return 0;
}

/* ---- access-nbody ---- */
typedef struct { double x, y, z, vx, vy, vz, mass; } Body;
static const double SOLAR_MASS = 4 * JS_PI * JS_PI;
static const double DAYS_PER_YEAR = 365.24;
static Body body_new(double x, double y, double z, double vx, double vy, double vz, double mass) {
  Body b = {x, y, z, vx, vy, vz, mass};
  return b;
}
static void offset_momentum(Body *b, double px, double py, double pz) {
  b->vx = -px / SOLAR_MASS;
  b->vy = -py / SOLAR_MASS;
  b->vz = -pz / SOLAR_MASS;
}
static Body nbody_jupiter(void) {
  return body_new(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,
                  1.66007664274403694e-03 * DAYS_PER_YEAR, 7.69901118419740425e-03 * DAYS_PER_YEAR,
                  -6.90460016972063023e-05 * DAYS_PER_YEAR, 9.54791938424326609e-04 * SOLAR_MASS);
}
static Body nbody_saturn(void) {
  return body_new(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,
                  -2.76742510726862411e-03 * DAYS_PER_YEAR, 4.99852801234917238e-03 * DAYS_PER_YEAR,
                  2.30417297573763929e-05 * DAYS_PER_YEAR, 2.85885980666130812e-04 * SOLAR_MASS);
}
static Body nbody_uranus(void) {
  return body_new(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01,
                  2.96460137564761618e-03 * DAYS_PER_YEAR, 2.37847173959480950e-03 * DAYS_PER_YEAR,
                  -2.96589568540237556e-05 * DAYS_PER_YEAR, 4.36624404335156298e-05 * SOLAR_MASS);
}
static Body nbody_neptune(void) {
  return body_new(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,
                  2.68067772490389322e-03 * DAYS_PER_YEAR, 1.62824170038242295e-03 * DAYS_PER_YEAR,
                  -9.51592254519715870e-05 * DAYS_PER_YEAR, 5.15138902046611451e-05 * SOLAR_MASS);
}
static Body nbody_sun(void) { return body_new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR_MASS); }

typedef struct { Body *bodies; int size; } NBodySystem;
static void nbody_system_init(NBodySystem *s, Body *bodies, int size) {
  s->bodies = bodies;
  s->size = size;
  double px = 0.0, py = 0.0, pz = 0.0;
  for (int i = 0; i < size; i++) {
    Body *b = &bodies[i];
    double m = b->mass;
    px += b->vx * m;
    py += b->vy * m;
    pz += b->vz * m;
  }
  offset_momentum(&bodies[0], px, py, pz);
}
static void nbody_advance(NBodySystem *sys, double dt) {
  Body *bodies = sys->bodies;
  int size = sys->size;
  double dx, dy, dz, distance, mag;
  for (int i = 0; i < size; i++) {
    Body *bodyi = &bodies[i];
    for (int j = i + 1; j < size; j++) {
      Body *bodyj = &bodies[j];
      dx = bodyi->x - bodyj->x;
      dy = bodyi->y - bodyj->y;
      dz = bodyi->z - bodyj->z;
      distance = sqrt(dx * dx + dy * dy + dz * dz);
      mag = dt / (distance * distance * distance);
      bodyi->vx -= dx * bodyj->mass * mag;
      bodyi->vy -= dy * bodyj->mass * mag;
      bodyi->vz -= dz * bodyj->mass * mag;
      bodyj->vx += dx * bodyi->mass * mag;
      bodyj->vy += dy * bodyi->mass * mag;
      bodyj->vz += dz * bodyi->mass * mag;
    }
  }
  for (int i = 0; i < size; i++) {
    Body *body = &bodies[i];
    body->x += dt * body->vx;
    body->y += dt * body->vy;
    body->z += dt * body->vz;
  }
}
static double nbody_energy(const NBodySystem *sys) {
  const Body *bodies = sys->bodies;
  int size = sys->size;
  double dx, dy, dz, distance;
  double e = 0.0;
  for (int i = 0; i < size; i++) {
    const Body *bodyi = &bodies[i];
    e += 0.5 * bodyi->mass * (bodyi->vx * bodyi->vx + bodyi->vy * bodyi->vy + bodyi->vz * bodyi->vz);
    for (int j = i + 1; j < size; j++) {
      const Body *bodyj = &bodies[j];
      dx = bodyi->x - bodyj->x;
      dy = bodyi->y - bodyj->y;
      dz = bodyi->z - bodyj->z;
      distance = sqrt(dx * dx + dy * dy + dz * dz);
      e -= (bodyi->mass * bodyj->mass) / distance;
    }
  }
  return e;
}
static int run_access_nbody(void) {
  double ret = 0;
  for (int n = 3; n <= 24; n *= 2) {
    Body bodies[5] = {nbody_sun(), nbody_jupiter(), nbody_saturn(), nbody_uranus(), nbody_neptune()};
    NBodySystem sys;
    nbody_system_init(&sys, bodies, 5);
    int max = n * 100;
    ret += nbody_energy(&sys);
    for (int i = 0; i < max; i++) nbody_advance(&sys, 0.01);
    ret += nbody_energy(&sys);
  }
  if (ret != -1.3524862408537381) return bomb_fail("access-nbody", "energy sum", ret, -1.3524862408537381);
  return 0;
}

/* ---- math-cordic ---- */
static double cordic_angles[12];
static double cordic_sincos(double Target) {
  const double AG_CONST = 0.6072529350;
  double X, Y, TargetAngle, CurrAngle;
  X = AG_CONST * 65536.0;
  Y = 0;
  TargetAngle = Target * 65536.0;
  CurrAngle = 0;
  for (int Step = 0; Step < 12; Step++) {
    double NewX;
    if (TargetAngle > CurrAngle) {
      NewX = X - (double)(to_int32(Y) >> Step);
      Y = (double)(to_int32(X) >> Step) + Y;
      X = NewX;
      CurrAngle += cordic_angles[Step];
    } else {
      NewX = X + (double)(to_int32(Y) >> Step);
      Y = -(double)(to_int32(X) >> Step) + Y;
      X = NewX;
      CurrAngle -= cordic_angles[Step];
    }
  }
  return (X / 65536.0) * (Y / 65536.0);
}
static int run_math_cordic(void) {
  static const double raw[12] = {45.0, 26.565, 14.0362, 7.12502, 3.57633, 1.78991,
                                 0.895174, 0.447614, 0.223811, 0.111906, 0.055953, 0.027977};
  for (int i = 0; i < 12; i++) cordic_angles[i] = raw[i] * 65536.0;
  double total = 0;
  for (int i = 0; i < 25000; i++) total += cordic_sincos(28.027);
  if (total != 10362.570468755888) return bomb_fail("math-cordic", "total", total, 10362.570468755888);
  return 0;
}

/* ---- math-partial-sums ---- */
static volatile double partial_sink;
static double partial_sums(int n) {
  double a1 = 0.0, a2 = 0.0, a3 = 0.0, a4 = 0.0, a5 = 0.0, a6 = 0.0, a7 = 0.0, a8 = 0.0, a9 = 0.0;
  double twothirds = 2.0 / 3.0;
  double alt = -1.0;
  double k2, k3, sk, ck;
  for (int k = 1; k <= n; k++) {
    k2 = k * k;
    k3 = k2 * k;
    sk = v8m_sin((double)k);
    ck = v8m_cos((double)k);
    alt = -alt;
    a1 += v8m_pow(twothirds, (double)(k - 1));
    a2 += v8m_pow((double)k, -0.5);
    a3 += 1.0 / (k * (k + 1.0));
    a4 += 1.0 / (k3 * sk * sk);
    a5 += 1.0 / (k3 * ck * ck);
    a6 += 1.0 / k;
    a7 += 1.0 / k2;
    a8 += alt / k;
    a9 += alt / (2 * k - 1);
  }
  /* The JS computes a1..a5 and does not return them; keep them live so they are computed. */
  partial_sink = a1 + a2 + a3 + a4 + a5;
  return a6 + a7 + a8 + a9;
}
static int run_math_partial_sums(void) {
  double total = 0;
  for (int i = 1024; i <= 16384; i *= 2) total += partial_sums(i);
  if (total != 60.08994194659945) return bomb_fail("math-partial-sums", "total", total, 60.08994194659945);
  return 0;
}

/* ---- math-spectral-norm ---- */
static double spectral_A(int i, int j) {
  return 1.0 / ((double)(i + j) * (double)(i + j + 1) / 2.0 + i + 1.0);
}
static void spectral_Au(const double *u, double *v, int n) {
  for (int i = 0; i < n; ++i) {
    double t = 0;
    for (int j = 0; j < n; ++j) t += spectral_A(i, j) * u[j];
    v[i] = t;
  }
}
static void spectral_Atu(const double *u, double *v, int n) {
  for (int i = 0; i < n; ++i) {
    double t = 0;
    for (int j = 0; j < n; ++j) t += spectral_A(j, i) * u[j];
    v[i] = t;
  }
}
static double spectral_norm(int n) {
  double *u = malloc(sizeof(double) * (size_t)n), *v = malloc(sizeof(double) * (size_t)n),
         *w = malloc(sizeof(double) * (size_t)n);
  double vv = 0, vBv = 0;
  for (int i = 0; i < n; ++i) { u[i] = 1; v[i] = w[i] = 0; }
  for (int i = 0; i < 10; ++i) {
    spectral_Au(u, w, n); spectral_Atu(w, v, n);   /* AtAu(u, v, w) */
    spectral_Au(v, w, n); spectral_Atu(w, u, n);   /* AtAu(v, u, w) */
  }
  for (int i = 0; i < n; ++i) {
    vBv += u[i] * v[i];
    vv += v[i] * v[i];
  }
  free(u); free(v); free(w);
  return sqrt(vBv / vv);
}
static int run_math_spectral_norm(void) {
  double total = 0;
  for (int i = 6; i <= 48; i *= 2) total += spectral_norm(i);
  if (total != 5.086694231303284) return bomb_fail("math-spectral-norm", "total", total, 5.086694231303284);
  return 0;
}

/* ---- 3d-morph ---- */
static void morph(double *a, double f) {
  const int nx = 120, nz = 120;
  double PI2nx = JS_PI * 8 / nx;
  double f30 = -(50 * v8m_sin(f * JS_PI * 2));
  for (int i = 0; i < nz; ++i)
    for (int j = 0; j < nx; ++j)
      a[3 * (i * nx + j) + 1] = v8m_sin((j - 1) * PI2nx) * -f30;
}
static int run_3d_morph(void) {
  const int loops = 15, nx = 120, nz = 120;
  double *a = calloc((size_t)(nx * nz * 3), sizeof(double));
  for (int i = 0; i < loops; ++i) morph(a, i / (double)loops);
  double testOutput = 0;
  for (int i = 0; i < nx; i++) testOutput += a[3 * (i * nx + i) + 1];
  free(a);
  if (fabs(testOutput) >= 1e-13) return bomb_fail("3d-morph", "magnitude", fabs(testOutput), 1e-13);
  return 0;
}

/* ---- 3d-cube: 3D Cube Rotation (Simon Speich), with the same matrices and line rasteriser ---- */
typedef struct { double v[4]; } Vec4;
typedef struct { double m[4][4]; } Mat4;
static Mat4 cube_identity(void) {
  Mat4 r = {{{1, 0, 0, 0}, {0, 1, 0, 0}, {0, 0, 1, 0}, {0, 0, 0, 1}}};
  return r;
}
static Mat4 mmulti(Mat4 M1, Mat4 M2) {
  Mat4 M;
  for (int i = 0; i < 4; i++)
    for (int j = 0; j < 4; j++)
      M.m[i][j] = M1.m[i][0] * M2.m[0][j] + M1.m[i][1] * M2.m[1][j] + M1.m[i][2] * M2.m[2][j] + M1.m[i][3] * M2.m[3][j];
  return M;
}
static Vec4 vmulti(Mat4 M, Vec4 V) {
  Vec4 r;
  for (int i = 0; i < 4; i++) r.v[i] = M.m[i][0] * V.v[0] + M.m[i][1] * V.v[1] + M.m[i][2] * V.v[2] + M.m[i][3] * V.v[3];
  return r;
}
static Vec4 vmulti2(Mat4 M, Vec4 V) {
  Vec4 r = {{0, 0, 0, 0}};
  for (int i = 0; i < 3; i++) r.v[i] = M.m[i][0] * V.v[0] + M.m[i][1] * V.v[1] + M.m[i][2] * V.v[2];
  return r;
}
static Mat4 cube_translate(Mat4 M, double Dx, double Dy, double Dz) {
  Mat4 T = {{{1, 0, 0, Dx}, {0, 1, 0, Dy}, {0, 0, 1, Dz}, {0, 0, 0, 1}}};
  return mmulti(T, M);
}
static Mat4 cube_rotate_x(Mat4 M, double Phi) {
  double a = Phi;
  a *= JS_PI / 180;
  double Cos = v8m_cos(a), Sin = v8m_sin(a);
  Mat4 R = {{{1, 0, 0, 0}, {0, Cos, -Sin, 0}, {0, Sin, Cos, 0}, {0, 0, 0, 1}}};
  return mmulti(R, M);
}
static Mat4 cube_rotate_y(Mat4 M, double Phi) {
  double a = Phi;
  a *= JS_PI / 180;
  double Cos = v8m_cos(a), Sin = v8m_sin(a);
  Mat4 R = {{{Cos, 0, Sin, 0}, {0, 1, 0, 0}, {-Sin, 0, Cos, 0}, {0, 0, 0, 1}}};
  return mmulti(R, M);
}
static Mat4 cube_rotate_z(Mat4 M, double Phi) {
  double a = Phi;
  a *= JS_PI / 180;
  double Cos = v8m_cos(a), Sin = v8m_sin(a);
  Mat4 R = {{{Cos, -Sin, 0, 0}, {Sin, Cos, 0, 0}, {0, 0, 1, 0}, {0, 0, 0, 1}}};
  return mmulti(R, M);
}
static Vec4 calc_normal(const Vec4 *V0, const Vec4 *V1, const Vec4 *V2) {
  double A[3], B[3];
  for (int i = 0; i < 3; i++) {
    A[i] = V0->v[i] - V1->v[i];
    B[i] = V2->v[i] - V1->v[i];
  }
  double C[3];
  C[0] = A[1] * B[2] - A[2] * B[1];
  C[1] = A[2] * B[0] - A[0] * B[2];
  C[2] = A[0] * B[1] - A[1] * B[0];
  double Length = sqrt(C[0] * C[0] + C[1] * C[1] + C[2] * C[2]);
  Vec4 r;
  for (int i = 0; i < 3; i++) r.v[i] = C[i] / Length;
  r.v[3] = 1;
  return r;
}

typedef struct {
  Vec4 Q[9];            /* Q[0..8]: the corners and the centre of gravity */
  Vec4 Normal[6];       /* Q.Normal */
  bool Line[12];        /* Q.Line */
  double LastPx;
  int NumPx;
  Vec4 *LinePixels;     /* Q.LinePixels: allocated, never read */
  Mat4 MTrans, MQube;
  int LoopCount, LoopMax;
  Vec4 Origin;
} Cube;

/* Math.round for the non-negative pixel counts used here. */
static void draw_line(Cube *c, const double *From, const double *To) {
  double x1 = From[0];
  double x2 = To[0];
  double y1 = From[1];
  double y2 = To[1];
  double dx = fabs(x2 - x1);
  double dy = fabs(y2 - y1);
  double x = x1;
  double y = y1;
  double IncX1, IncY1, IncX2, IncY2, Den, Num, NumAdd, NumPix;
  if (x2 >= x1) { IncX1 = 1; IncX2 = 1; } else { IncX1 = -1; IncX2 = -1; }
  if (y2 >= y1) { IncY1 = 1; IncY2 = 1; } else { IncY1 = -1; IncY2 = -1; }
  if (dx >= dy) {
    IncX1 = 0;
    IncY2 = 0;
    Den = dx;
    Num = dx / 2;
    NumAdd = dy;
    NumPix = dx;
  } else {
    IncX2 = 0;
    IncY1 = 0;
    Den = dy;
    Num = dy / 2;
    NumAdd = dx;
    NumPix = dy;
  }
  NumPix = js_round(c->LastPx + NumPix);
  double i = c->LastPx;
  for (; i < NumPix; i++) {
    Num += NumAdd;
    if (Num >= Den) {
      Num -= Den;
      x += IncX1;
      y += IncY1;
    }
    x += IncX2;
    y += IncY2;
  }
  c->LastPx = NumPix;
}

#define CUBE_EDGE(flag, a, b) \
  if (!c->Line[flag]) { draw_line(c, c->Q[a].v, c->Q[b].v); c->Line[flag] = true; }

static void draw_qube(Cube *c) {
  Vec4 CurN[6];
  c->LastPx = 0;
  for (int i = 5; i > -1; i--) CurN[i] = vmulti2(c->MQube, c->Normal[i]);
  if (CurN[0].v[2] < 0) {
    CUBE_EDGE(0, 0, 1);
    CUBE_EDGE(1, 1, 2);
    CUBE_EDGE(2, 2, 3);
    CUBE_EDGE(3, 3, 0);
  }
  if (CurN[1].v[2] < 0) {
    CUBE_EDGE(2, 3, 2);
    CUBE_EDGE(9, 2, 6);
    CUBE_EDGE(6, 6, 7);
    CUBE_EDGE(10, 7, 3);
  }
  if (CurN[2].v[2] < 0) {
    CUBE_EDGE(4, 4, 5);
    CUBE_EDGE(5, 5, 6);
    CUBE_EDGE(6, 6, 7);
    CUBE_EDGE(7, 7, 4);
  }
  if (CurN[3].v[2] < 0) {
    CUBE_EDGE(4, 4, 5);
    CUBE_EDGE(8, 5, 1);
    CUBE_EDGE(0, 1, 0);
    CUBE_EDGE(11, 0, 4);
  }
  if (CurN[4].v[2] < 0) {
    CUBE_EDGE(11, 4, 0);
    CUBE_EDGE(3, 0, 3);
    CUBE_EDGE(10, 3, 7);
    CUBE_EDGE(7, 7, 4);
  }
  if (CurN[5].v[2] < 0) {
    CUBE_EDGE(8, 1, 5);
    CUBE_EDGE(5, 5, 6);
    CUBE_EDGE(9, 6, 2);
    CUBE_EDGE(1, 2, 1);
  }
  for (int i = 0; i < 12; i++) c->Line[i] = false;
  c->LastPx = 0;
}

/* Loop(): runs while LoopCount <= LoopMax, as its recursion does. */
static void cube_loop(Cube *c) {
  while (!(c->LoopCount > c->LoopMax)) {
    char testing[16];
    int tl = snprintf(testing, sizeof testing, "%d", c->LoopCount);
    while (tl < 3) { memmove(testing + 1, testing, (size_t)tl + 1); testing[0] = '0'; tl++; }
    Mat4 MT = cube_identity();
    MT = cube_translate(MT, -c->Q[8].v[0], -c->Q[8].v[1], -c->Q[8].v[2]);
    MT = cube_rotate_x(MT, 1);
    MT = cube_rotate_y(MT, 3);
    MT = cube_rotate_z(MT, 5);
    MT = cube_translate(MT, c->Q[8].v[0], c->Q[8].v[1], c->Q[8].v[2]);
    c->MTrans = MT;
    c->MQube = mmulti(c->MTrans, c->MQube);
    for (int i = 8; i > -1; i--) c->Q[i] = vmulti(c->MTrans, c->Q[i]);
    draw_qube(c);
    c->LoopCount++;
  }
}

static double cube_init_run(int CubeSize) {
  Cube *c = calloc(1, sizeof *c);
  c->Origin = (Vec4){{150, 150, 20, 1}};
  c->LoopCount = 0;
  c->LoopMax = 50;
  c->MTrans = cube_identity();
  c->MQube = cube_identity();
  double cs = CubeSize;
  Vec4 corners[8] = {
    {{-cs, -cs, cs, 1}}, {{-cs, cs, cs, 1}}, {{cs, cs, cs, 1}}, {{cs, -cs, cs, 1}},
    {{-cs, -cs, -cs, 1}}, {{-cs, cs, -cs, 1}}, {{cs, cs, -cs, 1}}, {{cs, -cs, -cs, 1}},
  };
  for (int i = 0; i < 8; i++) c->Q[i] = corners[i];
  c->Q[8] = (Vec4){{0, 0, 0, 1}};
  static const int Edge[6][3] = {{0, 1, 2}, {3, 2, 6}, {7, 6, 5}, {4, 5, 1}, {4, 0, 3}, {1, 5, 6}};
  for (int i = 0; i < 6; i++) c->Normal[i] = calc_normal(&c->Q[Edge[i][0]], &c->Q[Edge[i][1]], &c->Q[Edge[i][2]]);
  for (int i = 0; i < 12; i++) c->Line[i] = false;
  c->NumPx = 9 * 2 * CubeSize;
  c->LinePixels = malloc(sizeof(Vec4) * (size_t)c->NumPx);
  for (int i = 0; i < c->NumPx; i++) c->LinePixels[i] = (Vec4){{0, 0, 0, 1}};
  c->MTrans = cube_translate(c->MTrans, c->Origin.v[0], c->Origin.v[1], c->Origin.v[2]);
  c->MQube = mmulti(c->MTrans, c->MQube);
  for (int i = 0; i < 9; i++) c->Q[i] = vmulti(c->MTrans, c->Q[i]);
  draw_qube(c);
  cube_loop(c);
  double sum = 0;
  for (int i = 0; i < 9; i++)
    for (int j = 0; j < 4; j++) sum += c->Q[i].v[j];
  free(c->LinePixels);
  free(c);
  return sum;
}

static int run_3d_cube(void) {
  static const int sizes[4] = {20, 40, 80, 160};
  static const double validation[4] = {2889.0000000000045, 2889.0000000000055, 2889.000000000005, 2889.0000000000055};
  int failures = 0;
  for (int k = 0; k < 4; k++) {
    double sum = cube_init_run(sizes[k]);
    if (sum != validation[k]) failures += bomb_fail("3d-cube", "vector sum", sum, validation[k]);
  }
  return failures;
}

/* ---- 3d-raytrace: a small ray tracer over a cube and a shaded floor (the JS Scene/Camera/Triangle) ---- */
typedef struct { double v[3]; } V3;
typedef struct { double c[3]; double reflection; } Colour;   /* reflection is NaN when the JS value is undefined */

static V3 v3(double x, double y, double z) { V3 r = {{x, y, z}}; return r; }
static V3 v3_add(V3 a, V3 b) { return v3(a.v[0] + b.v[0], a.v[1] + b.v[1], a.v[2] + b.v[2]); }
static V3 v3_sub(V3 a, V3 b) { return v3(a.v[0] - b.v[0], a.v[1] - b.v[1], a.v[2] - b.v[2]); }
static V3 v3_scale(V3 a, double s) { return v3(a.v[0] * s, a.v[1] * s, a.v[2] * s); }
static V3 v3_scalev(V3 a, const double *b) { return v3(a.v[0] * b[0], a.v[1] * b[1], a.v[2] * b[2]); }
static double v3_dot(V3 a, V3 b) { return a.v[0] * b.v[0] + a.v[1] * b.v[1] + a.v[2] * b.v[2]; }
static double v3_length(V3 a) { return sqrt(a.v[0] * a.v[0] + a.v[1] * a.v[1] + a.v[2] * a.v[2]); }
static V3 v3_normalise(V3 v) {
  double len = v3_length(v);
  return v3(v.v[0] / len, v.v[1] / len, v.v[2] / len);
}
static V3 v3_cross(V3 v1, V3 v2) {
  return v3(v1.v[1] * v2.v[2] - v1.v[2] * v2.v[1], v1.v[2] * v2.v[0] - v1.v[0] * v2.v[2], v1.v[0] * v2.v[1] - v1.v[1] * v2.v[0]);
}

typedef struct {
  int axis;
  V3 normal;
  double nu, nv, nd, eu, ev, nu1, nv1, nu2, nv2;
  Colour material;
  int shader;   /* 0: the triangle's material, 1: the floor shader */
} Triangle;

static Triangle triangle_new(V3 p1, V3 p2, V3 p3) {
  Triangle t;
  V3 edge1 = v3_sub(p3, p1);
  V3 edge2 = v3_sub(p2, p1);
  V3 normal = v3_cross(edge1, edge2);
  if (fabs(normal.v[0]) > fabs(normal.v[1])) {
    if (fabs(normal.v[0]) > fabs(normal.v[2])) t.axis = 0;
    else t.axis = 2;
  } else {
    if (fabs(normal.v[1]) > fabs(normal.v[2])) t.axis = 1;
    else t.axis = 2;
  }
  int u = (t.axis + 1) % 3;
  int v = (t.axis + 2) % 3;
  double u1 = edge1.v[u], v1 = edge1.v[v], u2 = edge2.v[u], v2 = edge2.v[v];
  t.normal = v3_normalise(normal);
  t.nu = normal.v[u] / normal.v[t.axis];
  t.nv = normal.v[v] / normal.v[t.axis];
  t.nd = v3_dot(normal, p1) / normal.v[t.axis];
  double det = u1 * v2 - v1 * u2;
  t.eu = p1.v[u];
  t.ev = p1.v[v];
  t.nu1 = u1 / det;
  t.nv1 = -v1 / det;
  t.nu2 = v2 / det;
  t.nv2 = -u2 / det;
  t.material = (Colour){{0.7, 0.7, 0.7}, NAN};
  t.shader = 0;
  return t;
}

/* Triangle.prototype.intersect: returns whether the ray hits, and the distance t. A NaN near or
 * far (the JS undefined) makes its comparison false, as in the JS. */
static bool triangle_intersect(const Triangle *tr, V3 orig, V3 dir, double near, double far, double *out) {
  int u = (tr->axis + 1) % 3;
  int v = (tr->axis + 2) % 3;
  double d = dir.v[tr->axis] + tr->nu * dir.v[u] + tr->nv * dir.v[v];
  double t = (tr->nd - orig.v[tr->axis] - tr->nu * orig.v[u] - tr->nv * orig.v[v]) / d;
  if (t < near || t > far) return false;
  double Pu = orig.v[u] + t * dir.v[u] - tr->eu;
  double Pv = orig.v[v] + t * dir.v[v] - tr->ev;
  double a2 = Pv * tr->nu1 + Pu * tr->nv1;
  if (a2 < 0) return false;
  double a3 = Pu * tr->nu2 + Pv * tr->nv2;
  if (a3 < 0) return false;
  if ((a2 + a3) > 1) return false;
  *out = t;
  return true;
}

static const Colour rt_grey = {{0.4, 0.4, 0.4}, 1.0};
static const Colour rt_green = {{0.0, 0.4, 0.0}, NAN};
static Colour floor_shader(V3 pos) {
  double x = fmod(fmod(pos.v[0] / 32, 2) + 2, 2);
  double z = fmod(fmod(pos.v[2] / 32 + 0.3, 2) + 2, 2);
  if ((x < 1) != (z < 1)) return rt_grey;
  return rt_green;
}

typedef struct {
  Triangle tris[14];
  int ntri;
  V3 lights[3], light_colour[3];
  V3 ambient, background;
} Scene;

static bool scene_blocked(const Scene *s, V3 O, V3 D, double far) {
  double near = 0.0001;
  for (int i = 0; i < s->ntri; i++) {
    double d;
    if (!triangle_intersect(&s->tris[i], O, D, near, far, &d)) continue;
    if (d > far || d < near) continue;
    return true;
  }
  return false;
}

static V3 scene_intersect(const Scene *s, V3 origin, V3 dir, double near, double far) {
  const Triangle *closest = NULL;
  for (int i = 0; i < s->ntri; i++) {
    const Triangle *triangle = &s->tris[i];
    double d;
    if (!triangle_intersect(triangle, origin, dir, near, far, &d)) continue;
    if (d > far || d < near) continue;
    far = d;
    closest = triangle;
  }
  if (!closest) return s->background;
  V3 normal = closest->normal;
  V3 hit = v3_add(origin, v3_scale(dir, far));
  if (v3_dot(dir, normal) > 0) normal = v3(-normal.v[0], -normal.v[1], -normal.v[2]);
  Colour colour = closest->shader == 1 ? floor_shader(hit) : closest->material;
  bool have_reflected = false;
  V3 reflected = v3(0, 0, 0);
  if (colour.reflection > 0.001) {
    V3 reflection = v3_add(v3_scale(normal, -2 * v3_dot(dir, normal)), dir);
    reflected = scene_intersect(s, hit, reflection, 0.0001, 1000000);
    have_reflected = true;
    if (colour.reflection >= 0.999999) return reflected;
  }
  V3 l = s->ambient;
  for (int i = 0; i < 3; i++) {
    V3 toLight = v3_sub(s->lights[i], hit);
    double distance = v3_length(toLight);
    toLight = v3_scale(toLight, 1.0 / distance);
    distance -= 0.0001;
    if (scene_blocked(s, hit, toLight, distance)) continue;
    double nl = v3_dot(normal, toLight);
    if (nl > 0) l = v3_add(l, v3_scale(s->light_colour[i], nl));
  }
  l = v3_scalev(l, colour.c);
  if (have_reflected) l = v3_add(v3_scale(l, 1 - colour.reflection), v3_scale(reflected, colour.reflection));
  return l;
}

typedef struct {
  V3 origin;
  V3 directions[4];
  double m[16];
} Camera;

static Camera camera_new(V3 origin, V3 lookat, V3 up) {
  Camera cam;
  V3 zaxis = v3_normalise(v3_sub(lookat, origin));
  V3 xaxis = v3_normalise(v3_cross(up, zaxis));
  V3 yaxis = v3_normalise(v3_cross(xaxis, v3_sub(v3(0, 0, 0), zaxis)));
  /* m = new Array(16): rows 0..2 of the transpose; invertMatrix leaves [3], [7], [11] as
   * undefined and then Camera sets them to 0, so they are 0 here. */
  double m[16] = {0};
  m[0] = xaxis.v[0]; m[1] = xaxis.v[1]; m[2] = xaxis.v[2];
  m[4] = yaxis.v[0]; m[5] = yaxis.v[1]; m[6] = yaxis.v[2];
  m[8] = zaxis.v[0]; m[9] = zaxis.v[1]; m[10] = zaxis.v[2];
  double temp[16] = {0};
  for (int h = 0; h < 3; h++)
    for (int v = 0; v < 3; v++) temp[h + v * 4] = m[v + h * 4];
  for (int i = 0; i < 11; i++) m[i] = temp[i];
  m[3] = 0; m[7] = 0; m[11] = 0;
  memcpy(cam.m, m, sizeof m);
  cam.origin = origin;
  cam.directions[0] = v3_normalise(v3(-0.7, 0.7, 1));
  cam.directions[1] = v3_normalise(v3(0.7, 0.7, 1));
  cam.directions[2] = v3_normalise(v3(0.7, -0.7, 1));
  cam.directions[3] = v3_normalise(v3(-0.7, -0.7, 1));
  for (int k = 0; k < 4; k++) {
    V3 d = cam.directions[k];
    const double *vals = cam.m;
    double x = vals[0] * d.v[0] + vals[1] * d.v[1] + vals[2] * d.v[2] + vals[3];
    double y = vals[4] * d.v[0] + vals[5] * d.v[1] + vals[6] * d.v[2] + vals[7];
    double z = vals[8] * d.v[0] + vals[9] * d.v[1] + vals[10] * d.v[2] + vals[11];
    cam.directions[k] = v3(x, y, z);
  }
  return cam;
}

static void render_rows(const Camera *cam, const Scene *scene, V3 pixels[30][30], int width, int height, int starty, int stopy) {
  for (int y = starty; y < stopy; y++) {
    double yf = y / (double)height;
    V3 dir0 = v3_add(v3_scale(cam->directions[0], yf), v3_scale(cam->directions[3], 1 - yf));
    V3 dir1 = v3_add(v3_scale(cam->directions[1], yf), v3_scale(cam->directions[2], 1 - yf));
    for (int x = 0; x < width; x++) {
      double xp = x / (double)width;
      V3 origin = v3_add(v3_scale(cam->origin, xp), v3_scale(cam->origin, 1 - xp));
      V3 dir = v3_normalise(v3_add(v3_scale(dir0, xp), v3_scale(dir1, 1 - xp)));
      pixels[y][x] = scene_intersect(scene, origin, dir, NAN, NAN);
    }
  }
}

static void raytrace_scene(V3 pixels[30][30]) {
  V3 tfl = v3(-10, 10, -10), tfr = v3(10, 10, -10), tbl = v3(-10, 10, 10), tbr = v3(10, 10, 10);
  V3 bfl = v3(-10, -10, -10), bfr = v3(10, -10, -10), bbl = v3(-10, -10, 10), bbr = v3(10, -10, 10);
  Scene scene;
  int i = 0;
  scene.tris[i++] = triangle_new(tfl, tfr, bfr);
  scene.tris[i++] = triangle_new(tfl, bfr, bfl);
  scene.tris[i++] = triangle_new(tbl, tbr, bbr);
  scene.tris[i++] = triangle_new(tbl, bbr, bbl);
  scene.tris[i++] = triangle_new(tbl, tfl, bbl);
  scene.tris[i++] = triangle_new(tfl, bfl, bbl);
  scene.tris[i++] = triangle_new(tbr, tfr, bbr);
  scene.tris[i++] = triangle_new(tfr, bfr, bbr);
  scene.tris[i++] = triangle_new(tbl, tbr, tfr);
  scene.tris[i++] = triangle_new(tbl, tfr, tfl);
  scene.tris[i++] = triangle_new(bbl, bbr, bfr);
  scene.tris[i++] = triangle_new(bbl, bfr, bfl);
  V3 ffl = v3(-1000, -30, -1000), ffr = v3(1000, -30, -1000), fbl = v3(-1000, -30, 1000), fbr = v3(1000, -30, 1000);
  scene.tris[i] = triangle_new(fbl, fbr, ffr);
  scene.tris[i++].shader = 1;
  scene.tris[i] = triangle_new(fbl, ffr, ffl);
  scene.tris[i++].shader = 1;
  scene.ntri = i;
  scene.lights[0] = v3(20, 38, -22);
  scene.light_colour[0] = v3(0.7, 0.3, 0.3);
  scene.lights[1] = v3(-23, 40, 17);
  scene.light_colour[1] = v3(0.7, 0.3, 0.3);
  scene.lights[2] = v3(23, 20, 17);
  scene.light_colour[2] = v3(0.7, 0.7, 0.7);
  scene.ambient = v3(0.1, 0.1, 0.1);
  scene.background = v3(0.8, 0.8, 1);
  const int size = 30;
  for (int y = 0; y < size; y++)
    for (int x = 0; x < size; x++) pixels[y][x] = v3(0, 0, 0);
  Camera cam = camera_new(v3(-40, 40, 40), v3(0, 0, 0), v3(0, 1, 0));
  render_rows(&cam, &scene, pixels, size, size, 0, size);
}

static void array_to_canvas_commands(V3 pixels[30][30], Str *s) {
  s->n = 0;
  if (!s->p) str_grow(s, 1);
  s->p[0] = 0;
  str_cat(s, "<canvas id=\"renderCanvas\" width=\"30px\" height=\"30px\"></canvas><script>\nvar pixels = [");
  const int size = 30;
  for (int y = 0; y < size; y++) {
    str_cat(s, "[");
    for (int x = 0; x < size; x++) {
      str_cat(s, "[");
      for (int k = 0; k < 3; k++) {
        if (k) str_char(s, ',');
        str_num(s, pixels[y][x].v[k]);
      }
      str_cat(s, "],");
    }
    str_cat(s, "],");
  }
  str_cat(s, "];\n    var canvas = document.getElementById(\"renderCanvas\").getContext(\"2d\");\n\n\n    var size = 30;\n    canvas.fillStyle = \"red\";\n    canvas.fillRect(0, 0, size, size);\n    canvas.scale(1, -1);\n    canvas.translate(0, -size);\n\n    if (!canvas.setFillColor)\n        canvas.setFillColor = function(r, g, b, a) {\n            this.fillStyle = \"rgb(\"+[Math.floor(r * 255), Math.floor(g * 255), Math.floor(b * 255)]+\")\";\n    }\n\nfor (var y = 0; y < size; y++) {\n  for (var x = 0; x < size; x++) {\n    var l = pixels[y][x];\n    canvas.setFillColor(l[0], l[1], l[2], 1);\n    canvas.fillRect(x, y, 1, 1);\n  }\n}</script>");
}

static int run_3d_raytrace(void) {
  static V3 pixels[30][30];
  raytrace_scene(pixels);
  Str s = {0};
  array_to_canvas_commands(pixels, &s);
  int failures = 0;
  if (s.n != 20970) failures += bomb_fail("3d-raytrace", "output length", (double)s.n, 20970);
  str_free(&s);
  return failures;
}

/* ---- string-base64: toBase64 / base64ToString as in the JS, with its toBinaryTable (index & 0x7f;
 * '=' maps to 0 in the table, and the padding flag keeps it from being appended) ---- */
static const char b64_table[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
static int b64_binary(unsigned c7) {
  if (c7 >= 'A' && c7 <= 'Z') return (int)(c7 - 'A');
  if (c7 >= 'a' && c7 <= 'z') return (int)(c7 - 'a') + 26;
  if (c7 >= '0' && c7 <= '9') return (int)(c7 - '0') + 52;
  if (c7 == '+') return 62;
  if (c7 == '/') return 63;
  if (c7 == '=') return 0;
  return -1;
}
static void to_base64(const Str *data, Str *result) {
  size_t length = data->n, i;
  const unsigned char *d = (const unsigned char *)data->p;
  result->n = 0;
  for (i = 0; i + 2 < length; i += 3) {
    str_char(result, b64_table[d[i] >> 2]);
    str_char(result, b64_table[((d[i] & 0x03) << 4) + (d[i + 1] >> 4)]);
    str_char(result, b64_table[((d[i + 1] & 0x0f) << 2) + (d[i + 2] >> 6)]);
    str_char(result, b64_table[d[i + 2] & 0x3f]);
  }
  if (length % 3) {
    i = length - (length % 3);
    str_char(result, b64_table[d[i] >> 2]);
    if (length % 3 == 2) {
      str_char(result, b64_table[((d[i] & 0x03) << 4) + (d[i + 1] >> 4)]);
      str_char(result, b64_table[(d[i + 1] & 0x0f) << 2]);
      str_char(result, '=');
    } else {
      str_char(result, b64_table[(d[i] & 0x03) << 4]);
      str_char(result, '=');
      str_char(result, '=');
    }
  }
}
static int base64_to_string(const Str *data, Str *result) {
  int leftbits = 0, leftdata = 0;
  result->n = 0;
  for (size_t i = 0; i < data->n; i++) {
    unsigned ch = (unsigned char)data->p[i];
    int c = b64_binary(ch & 0x7f);
    bool padding = ch == '=';
    if (c == -1) continue;
    leftdata = (leftdata << 6) | c;
    leftbits += 6;
    if (leftbits >= 8) {
      leftbits -= 8;
      if (!padding) str_char(result, (char)((leftdata >> leftbits) & 0xff));
      leftdata &= (1 << leftbits) - 1;
    }
  }
  if (leftbits) { fprintf(stderr, "Corrupted base64 string\n"); return 1; }
  return 0;
}
static int run_string_base64(void) {
  Str str = {0}, base64 = {0}, encoded = {0};
  int failures = 0;
  for (int i = 0; i < 8192; i++) str_char(&str, (char)(97 + (int)(25.0 * rng_next())));
  for (int i = 8192; i <= 16384; i *= 2) {
    to_base64(&str, &base64);
    failures += base64_to_string(&base64, &encoded);
    if (encoded.n != str.n || memcmp(encoded.p, str.p, str.n) != 0) {
      fprintf(stderr, "string-base64: bad result: round trip of %zu bytes differs\n", str.n);
      failures++;
    }
    str_double(&str);  /* str += str */
  }
  str_free(&str); str_free(&base64); str_free(&encoded);
  return failures;
}

/* Concatenated JS string literals ("a" + "b" + ...), read from p up to the ';' that ends the
 * statement. The decoded text is appended to out. */
static void concat_literals(const char *p, Str *out) {
  for (;;) {
    while (*p == ' ' || *p == '\n' || *p == '\t' || *p == '\r' || *p == '+') p++;
    if (*p == ';' || !*p) break;
    if (*p != '"') { fprintf(stderr, "unexpected token in literal list\n"); exit(2); }
    Str part = {0};
    js_literal(p + 1, '"', &part, &p);
    p++;
    str_add(out, part.p ? part.p : "", part.n);
    str_free(&part);
  }
}
/* ---- string-fasta: rand() is the shootout LCG (last is reset per run) ---- */
static long fasta_last = 42;
static double fasta_rand(void) {
  fasta_last = (fasta_last * 3877 + 29573) % 139968;
  return (double)fasta_last / 139968.0;
}
/* fastaRepeat: counts the characters the JS builds from seq, one 60-character line at a time. */
static double fasta_repeat(long n, long L) {
  long seqi = 0, lenOut = 60;
  double ret = 0;
  while (n > 0) {
    if (n < lenOut) lenOut = n;
    if (seqi + lenOut < L) {
      ret += (double)lenOut;               /* seq.substring(seqi, seqi+lenOut).length */
      seqi += lenOut;
    } else {
      long slen = L - seqi;                /* s = seq.substring(seqi) */
      seqi = lenOut - slen;
      ret += (double)(slen + seqi);        /* (s + seq.substring(0, seqi)).length */
    }
    n -= lenOut;
  }
  return ret;
}
/* fastaRandom: line[i] stays set from earlier lines (a holes-preserving array), and join() counts
 * only the set entries; `cum` is the makeCumulative table in the JS table's key order. */
static double fasta_random(long n, const double *cum, int nk) {
  bool def[60];
  int len = 60;
  double ret = 0;
  memset(def, 0, sizeof def);
  while (n > 0) {
    if (n < len) { len = (int)n; memset(def, 0, sizeof def); }   /* line = new Array(n) */
    for (int i = 0; i < len; i++) {
      double r = fasta_rand();
      for (int c = 0; c < nk; c++) {
        if (r < cum[c]) { def[i] = true; break; }
      }
    }
    int cnt = 0;
    for (int i = 0; i < len; i++) cnt += def[i] ? 1 : 0;   /* line.join('').length */
    ret += cnt;
    n -= len;
  }
  return ret;
}
static long fasta_alu_length(void) {
  Str alu = {0};
  concat_literals(find_after(S.fasta, "var ALU ="), &alu);
  long n = (long)alu.n;
  str_free(&alu);
  return n;
}
static int run_string_fasta(void) {
  double iub[15] = {0.27, 0.12, 0.12, 0.27, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02};
  double homo[4] = {0.3029549426680, 0.1979883004921, 0.1975473066391, 0.3015094502008};
  for (int k = 1; k < 15; k++) iub[k] += iub[k - 1];        /* makeCumulative */
  for (int k = 1; k < 4; k++) homo[k] += homo[k - 1];
  fasta_last = 42;
  double ret = 0;
  int count = 7;
  ret += fasta_repeat(2L * count * 100000, fasta_alu_length());
  ret += fasta_random(3L * count * 1000, iub, 15);
  ret += fasta_random(5L * count * 1000, homo, 4);
  double expected = 1456000;
  if (ret != expected) return bomb_fail("string-fasta", "length", ret, expected);
  return 0;
}

/* ---- string-validate-input: the JS regex /^[a-zA-Z0-9\-\._]+@[a-zA-Z0-9\-_]+(\.?[a-zA-Z0-9\-_]*)\.[a-zA-Z]{2,3}$/
 * is matched by hand: a local part of the class, one '@', then a domain of the class with at most one
 * '.' before a final '.' and 2 or 3 letters. The JS has no check on the result (its FIXME), so none here. ---- */
static bool vi_domain_char(unsigned char c) { return isalnum(c) || c == '-' || c == '_'; }
static bool vi_email_ok(const char *s, size_t n) {
  const char *at = memchr(s, '@', n);
  if (!at) return false;
  size_t ln = (size_t)(at - s);
  if (ln == 0) return false;
  for (size_t i = 0; i < ln; i++) {
    unsigned char c = (unsigned char)s[i];
    if (!(isalnum(c) || c == '-' || c == '.' || c == '_')) return false;
  }
  const char *d = at + 1;
  size_t dn = n - ln - 1;
  for (int t = 2; t <= 3; t++) {
    if (dn < (size_t)t + 2) continue;
    size_t dot = dn - (size_t)t - 1;
    if (d[dot] != '.') continue;
    bool letters = true;
    for (size_t k = dot + 1; k < dn; k++) if (!isalpha((unsigned char)d[k])) letters = false;
    if (!letters) continue;
    if (d[0] == '.') continue;
    int dots = 0;
    bool ok = true;
    for (size_t i = 0; i < dot && ok; i++) {
      if (d[i] == '.') dots++;
      else if (!vi_domain_char((unsigned char)d[i])) ok = false;
    }
    if (ok && dots <= 1) return true;
  }
  return false;
}
static const char vi_letters[] = "abcdefghijklmnopqrstuvwxyz";
static int run_string_validate_input(void) {
  Str endResult = {0}, r = {0};
  for (int k = 0; k < 4000; k++) {
    char username[7];
    for (int i = 0; i < 6; i++) username[i] = vi_letters[(int)floor(26 * rng_next())];
    username[6] = 0;
    Str email = {0};
    str_cat(&email, username);
    if (k % 2) str_cat(&email, "@mac.com");
    else str_cat(&email, "(at)mac.com");
    r.n = 0;
    str_add(&r, email.p, email.n);
    if (vi_email_ok(email.p, email.n)) str_cat(&r, " appears to be a valid email address.");
    else str_cat(&r, " does NOT appear to be a valid email address.");
    str_char(&endResult, '\n');
    str_add(&endResult, r.p, r.n);
    str_free(&email);
  }
  for (int s = 0; s < 4000; s++) {
    bool zipGood = true;
    Str zip = {0};
    for (int i = 0; i < 4; i++) str_char(&zip, (char)('0' + (int)floor(9 * rng_next())));
    if (s % 2) str_cat(&zip, "xyz");
    else str_cat(&zip, "7");
    for (size_t i = 0; i < zip.n; i++) {
      char ch = zip.p[i];
      if (ch < '0' || ch > '9') {
        zipGood = false;
        r.n = 0;
        str_add(&r, zip.p, zip.n);
        str_cat(&r, " contains letters.");
        str_char(&endResult, '\n');
        str_add(&endResult, r.p, r.n);
      }
    }
    if (zipGood && zip.n > 5) {
      zipGood = false;
      r.n = 0;
      str_add(&r, zip.p, zip.n);
      str_cat(&r, " is longer than five characters.");
      str_char(&endResult, '\n');
      str_add(&endResult, r.p, r.n);
    }
    if (zipGood) {
      r.n = 0;
      str_add(&r, zip.p, zip.n);
      str_cat(&r, " appears to be a valid ZIP code.");
      str_char(&endResult, '\n');
      str_add(&endResult, r.p, r.n);
    }
    str_free(&zip);
  }
  str_free(&r);
  str_free(&endResult);
  return 0;
}

/* ---- string-tagcloud: JSON.parse with the popularity reviver, a stable sort by tag, and the markup ---- */
typedef struct { char *tag; size_t tlen; double popularity; } TagInfo;
typedef struct { TagInfo *items; size_t n, cap; } TagList;

static void json_fail(const char *what) { fprintf(stderr, "SyntaxError: parseJSON (%s)\n", what); exit(2); }
static void jws(const char **p) { while (**p == ' ' || **p == '\t' || **p == '\n' || **p == '\r') (*p)++; }
/* A JSON string at *p (which must be '"'): decoded into out (ASCII only, as the data is). */
static void json_string(const char **p, Str *out) {
  if (**p != '"') json_fail("string");
  (*p)++;
  out->n = 0;
  for (;;) {
    unsigned char c = (unsigned char)**p;
    if (c == 0 || c < 0x20) json_fail("control character in string");
    if (c == '"') { (*p)++; break; }
    if (c != '\\') { str_char(out, (char)c); (*p)++; continue; }
    (*p)++;
    char e = **p;
    (*p)++;
    switch (e) {
      case '"': str_char(out, '"'); break;
      case '\\': str_char(out, '\\'); break;
      case '/': str_char(out, '/'); break;
      case 'b': str_char(out, '\b'); break;
      case 'f': str_char(out, '\f'); break;
      case 'n': str_char(out, '\n'); break;
      case 'r': str_char(out, '\r'); break;
      case 't': str_char(out, '\t'); break;
      case 'u': {
        unsigned cp = 0;
        for (int k = 0; k < 4; k++) {
          char h = **p;
          int d = isdigit((unsigned char)h) ? h - '0' : (h >= 'a' && h <= 'f') ? h - 'a' + 10 : (h >= 'A' && h <= 'F') ? h - 'A' + 10 : -1;
          if (d < 0) json_fail("bad \\u escape");
          cp = cp * 16 + (unsigned)d;
          (*p)++;
        }
        if (cp >= 0x80) { fprintf(stderr, "non-ASCII tag text is not in this data\n"); exit(2); }
        str_char(out, (char)cp);
        break;
      }
      default: json_fail("bad escape");
    }
  }
}
/* A JSON number at *p; the JSON grammar is checked before strtod (correctly rounded, as the engine). */
static double json_number(const char **p) {
  const char *s = *p;
  if (*s == '-') s++;
  if (*s == '0') s++;
  else if (*s >= '1' && *s <= '9') { while (isdigit((unsigned char)*s)) s++; }
  else json_fail("number");
  if (*s == '.') { s++; if (!isdigit((unsigned char)*s)) json_fail("fraction"); while (isdigit((unsigned char)*s)) s++; }
  if (*s == 'e' || *s == 'E') {
    s++;
    if (*s == '+' || *s == '-') s++;
    if (!isdigit((unsigned char)*s)) json_fail("exponent");
    while (isdigit((unsigned char)*s)) s++;
  }
  char buf[64];
  size_t n = (size_t)(s - *p);
  if (n >= sizeof buf) json_fail("number too long");
  memcpy(buf, *p, n);
  buf[n] = 0;
  *p = s;
  return strtod(buf, NULL);
}
static void json_skip_value(const char **p);
static void json_skip_value(const char **p) {
  Str tmp = {0};
  jws(p);
  char c = **p;
  if (c == '"') json_string(p, &tmp);
  else if (c == '-' || isdigit((unsigned char)c)) json_number(p);
  else if (!strncmp(*p, "true", 4)) *p += 4;
  else if (!strncmp(*p, "false", 5)) *p += 5;
  else if (!strncmp(*p, "null", 4)) *p += 4;
  else if (c == '[') {
    (*p)++;
    jws(p);
    if (**p == ']') { (*p)++; }
    else for (;;) {
      json_skip_value(p);
      jws(p);
      if (**p == ',') { (*p)++; continue; }
      if (**p == ']') { (*p)++; break; }
      json_fail("array");
    }
  } else if (c == '{') {
    (*p)++;
    jws(p);
    if (**p == '}') { (*p)++; }
    else for (;;) {
      json_string(p, &tmp);
      jws(p);
      if (**p != ':') json_fail("colon");
      (*p)++;
      json_skip_value(p);
      jws(p);
      if (**p == ',') { (*p)++; jws(p); continue; }
      if (**p == '}') { (*p)++; break; }
      json_fail("object");
    }
  } else json_fail("value");
  str_free(&tmp);
}
static void tag_list_push(TagList *l, TagInfo t) {
  if (l->n == l->cap) {
    l->cap = l->cap ? l->cap * 2 : 1024;
    l->items = realloc(l->items, l->cap * sizeof *l->items);
    if (!l->items) { fprintf(stderr, "out of memory\n"); exit(2); }
  }
  l->items[l->n++] = t;
}
/* JSON.parse(text, reviver) for the array of {tag, popularity} objects. The reviver is
 * function(k, v) { if (k == "popularity") return Math.log(v) / Math.log(2); return v; }, which runs on
 * the parsed values of each object, so it is applied where the key is read. */
static void parse_tag_info(const char *text, TagList *out) {
  const double log2v = v8m_log(2.0);
  const char *p = text;
  Str key = {0}, tag = {0};
  jws(&p);
  if (*p != '[') json_fail("top level");
  p++;
  jws(&p);
  if (*p == ']') { p++; }
  else for (;;) {
    jws(&p);
    if (*p != '{') json_fail("object in array");
    p++;
    TagInfo t = {NULL, 0, 0};
    bool have_tag = false, have_pop = false;
    jws(&p);
    if (*p == '}') json_fail("empty tag object");
    for (;;) {
      jws(&p);
      json_string(&p, &key);
      jws(&p);
      if (*p != ':') json_fail("colon");
      p++;
      jws(&p);
      if (key.n == 3 && !memcmp(key.p, "tag", 3)) {
        json_string(&p, &tag);
        t.tag = malloc(tag.n + 1);
        memcpy(t.tag, tag.p, tag.n);
        t.tag[tag.n] = 0;
        t.tlen = tag.n;
        have_tag = true;
      } else if (key.n == 10 && !memcmp(key.p, "popularity", 10)) {
        if (*p != '-' && !isdigit((unsigned char)*p)) json_fail("popularity is not a number");
        double v = json_number(&p);
        t.popularity = v8m_log(v) / log2v;
        have_pop = true;
      } else {
        json_skip_value(&p);
      }
      jws(&p);
      if (*p == ',') { p++; continue; }
      if (*p == '}') { p++; break; }
      json_fail("object member");
    }
    if (!have_tag || !have_pop) json_fail("missing tag or popularity");
    tag_list_push(out, t);
    jws(&p);
    if (*p == ',') { p++; continue; }
    if (*p == ']') { p++; break; }
    json_fail("array element");
  }
  jws(&p);
  if (*p) json_fail("trailing text");
  str_free(&key);
  str_free(&tag);
}
/* Lexicographic comparison of the tags (the JS string < on ASCII). */
static int tag_cmp(const TagInfo *a, const TagInfo *b) {
  size_t n = a->tlen < b->tlen ? a->tlen : b->tlen;
  int c = memcmp(a->tag, b->tag, n);
  if (c) return c < 0 ? -1 : 1;
  return a->tlen == b->tlen ? 0 : (a->tlen < b->tlen ? -1 : 1);
}
/* Stable merge sort (Array.prototype.sort is stable in the engine). */
static void tag_sort(TagInfo *a, size_t n) {
  TagInfo *tmp = malloc(n * sizeof *a);
  for (size_t width = 1; width < n; width *= 2) {
    for (size_t lo = 0; lo < n; lo += 2 * width) {
      size_t mid = lo + width < n ? lo + width : n, hi = lo + 2 * width < n ? lo + 2 * width : n;
      size_t i = lo, j = mid, k = lo;
      while (i < mid && j < hi) tmp[k++] = tag_cmp(&a[j], &a[i]) < 0 ? a[j++] : a[i++];
      while (i < mid) tmp[k++] = a[i++];
      while (j < hi) tmp[k++] = a[j++];
    }
    memcpy(a, tmp, n * sizeof *a);
  }
  free(tmp);
}
static void make_tag_cloud(TagList *list, Str *output) {
  output->n = 0;
  str_cat(output, "<div class=\"tagCloud\" style=\"width: 100%\">");
  tag_sort(list->items, list->n);
  for (size_t i = 0; i < list->n; i++) {
    TagInfo *t = &list->items[i];
    bool validates = true;
    for (size_t j = 0; j < t->tlen; j++) {
      unsigned char ch = (unsigned char)t->tag[j];
      if (ch < 0x20 || ch >= 0x7f) { validates = false; break; }
    }
    if (!validates) continue;
    /* "http://example.com/tag/" + tag.replace(" ", "").toLowerCase(): the first space only */
    Str url = {0};
    str_cat(&url, "http://example.com/tag/");
    bool removed = false;
    for (size_t j = 0; j < t->tlen; j++) {
      char ch = t->tag[j];
      if (!removed && ch == ' ') { removed = true; continue; }
      str_char(&url, (char)tolower((unsigned char)ch));
    }
    double popularity = t->popularity;
    Str color = {0};
    str_cat(&color, "rgb(");
    str_num(&color, floor(255 * (popularity - 12) / 20));
    str_cat(&color, ", 0, 255)");
    str_cat(output, " <a href=\"");
    str_add(output, url.p, url.n);
    str_cat(output, "\" style=\"font-size: ");
    str_num(output, popularity);
    str_cat(output, "px; color: ");
    str_add(output, color.p, color.n);
    str_cat(output, "\">");
    str_add(output, t->tag, t->tlen);
    str_cat(output, "</a> \n");
    str_free(&url);
    str_free(&color);
  }
  str_cat(output, "</div>");
  /* output.replace(" ", "&nbsp;") is evaluated and its result dropped by the JS. */
  Str discarded = {0};
  const char *sp = memchr(output->p, ' ', output->n);
  size_t before = sp ? (size_t)(sp - output->p) : output->n;
  str_add(&discarded, output->p, before);
  if (sp) { str_cat(&discarded, "&nbsp;"); str_add(&discarded, sp + 1, output->n - before - 1); }
  str_free(&discarded);
}
static int run_string_tagcloud(void) {
  Str text = {0};
  js_literal(find_after(S.tagcloud, "var tagInfoJSON = '"), '\'', &text, NULL);
  TagList list = {0};
  parse_tag_info(text.p, &list);
  str_free(&text);
  Str output = {0};
  make_tag_cloud(&list, &output);
  int failures = 0;
  double expectedMinLength = 315000;
  if ((double)output.n < expectedMinLength) failures += bomb_fail("string-tagcloud", "length", (double)output.n, expectedMinLength);
  for (size_t i = 0; i < list.n; i++) free(list.items[i].tag);
  free(list.items);
  str_free(&output);
  return failures;
}

/* ---- string-unpack-code: the P.A.C.K.E.R. unpacker. The JS (with ''.replace(/^/,String) === '' true in
 * the engine) builds d[token(c)] = k[c] || token(c) for c < count, then replaces every \w+ run of p by
 * d[run] (or the text "undefined" when there is no entry). token(c) is the base-a digit string of c. ---- */
static bool is_word_char(unsigned char c) { return isalnum(c) || c == '_'; }
static void packer_token(long c, int a, Str *out) {
  if (c >= a) packer_token(c / a, a, out);
  long r = c % a;
  if (r > 35) str_char(out, (char)(r + 29));
  else str_char(out, (char)(r < 10 ? '0' + r : 'a' + (r - 10)));
}
/* The index c whose token is the word w, or -1 when w is not a token of this dictionary. */
static long packer_index(const char *w, size_t wn, int a, long count) {
  if (wn == 0 || wn > 10) return -1;
  long v = 0;
  for (size_t i = 0; i < wn; i++) {
    unsigned char ch = (unsigned char)w[i];
    long d;
    if (ch >= '0' && ch <= '9') d = ch - '0';
    else if (ch >= 'a' && ch <= 'z') d = ch - 'a' + 10;
    else if (ch >= 'A' && ch <= 'Z') d = ch - 'A' + 36;
    else return -1;
    if (d >= a) return -1;
    v = v * a + d;
  }
  if (v >= count) return -1;
  Str t = {0};
  packer_token(v, a, &t);
  bool same = t.n == wn && memcmp(t.p, w, wn) == 0;
  str_free(&t);
  return same ? v : -1;
}
typedef struct { size_t off, len; } Piece;
static void unpack_packed(const Str *p, int a, long count, const Str *dict, Str *out) {
  /* dictionary pieces: dict.split('|') */
  Piece *pieces = malloc(((size_t)count + 1 + dict->n) * sizeof *pieces);
  size_t npieces = 0, start = 0;
  for (size_t i = 0; i <= dict->n; i++) {
    if (i == dict->n || dict->p[i] == '|') {
      pieces[npieces].off = start;
      pieces[npieces].len = i - start;
      npieces++;
      start = i + 1;
    }
  }
  out->n = 0;
  size_t i = 0;
  while (i < p->n) {
    if (!is_word_char((unsigned char)p->p[i])) { str_char(out, p->p[i++]); continue; }
    size_t j = i;
    while (j < p->n && is_word_char((unsigned char)p->p[j])) j++;
    long c = packer_index(p->p + i, j - i, a, count);
    if (c < 0) str_cat(out, "undefined");
    else if ((size_t)c < npieces && pieces[c].len) str_add(out, dict->p + pieces[c].off, pieces[c].len);
    else str_add(out, p->p + i, j - i);
    i = j;
  }
  free(pieces);
}
/* One packed call: }('packed',a,count,'dict'.split('|'),0,{}) ; returns the position after it. */
static const char *unpack_site(const char *cur, Str *packed, Str *dict, int *a, long *count) {
  const char *end;
  cur = find_after(cur, "}('");
  js_literal(cur, '\'', packed, &end);
  cur = end + 1;
  if (*cur != ',') { fprintf(stderr, "unpack: bad call\n"); exit(2); }
  char *e;
  *a = (int)strtol(cur + 1, &e, 10);
  if (*e != ',') { fprintf(stderr, "unpack: bad call\n"); exit(2); }
  *count = strtol(e + 1, &e, 10);
  if (strncmp(e, ",'", 2)) { fprintf(stderr, "unpack: bad call\n"); exit(2); }
  js_literal(e + 2, '\'', dict, &end);
  return end + 1;
}
static int run_string_unpack_code(void) {
  long result = 0;
  for (int iter = 0; iter < 2; iter++) {
    const char *cur = S.unpack;
    for (int lib = 0; lib < 4; lib++) {
      Str packed = {0}, dict = {0}, out = {0};
      int a;
      long count;
      cur = unpack_site(cur, &packed, &dict, &a, &count);
      unpack_packed(&packed, a, count, &dict, &out);
      result += (long)out.n;
      str_free(&packed); str_free(&dict); str_free(&out);
    }
  }
  long expected = 511508;
  if (result != expected) return bomb_fail("string-unpack-code", "length", (double)result, (double)expected);
  return 0;
}

/* ---- regexp-dna: nine case-insensitive alternations of fixed length, counted with /g (non-overlapping,
 * leftmost, first alternative wins), then eleven first-occurrence string replacements, and two exact
 * comparisons. The pattern sources are the JS regex sources. ---- */
static const char *dna_sources[9] = {
  "agggtaaa|tttaccct",
  "[cgt]gggtaaa|tttaccc[acg]",
  "a[act]ggtaaa|tttacc[agt]t",
  "ag[act]gtaaa|tttac[agt]ct",
  "agg[act]taaa|ttta[agt]cct",
  "aggg[acg]aaa|ttt[cgt]ccct",
  "agggt[cgt]aa|tt[acg]accct",
  "agggta[cgt]a|t[acg]taccct",
  "agggtaa[cgt]|[acg]ttaccct",
};
typedef struct { uint8_t m[16]; int len; } DnaAlt;      /* one alternative: per-position letter masks */
typedef struct { DnaAlt alt[2]; int nalt; } DnaPattern;
static uint8_t dna_bit(char ch) {                        /* a=1 c=2 g=4 t=8; other letters 0 */
  switch (ch) { case 'a': return 1; case 'c': return 2; case 'g': return 4; case 't': return 8; default: return 0; }
}
/* Parses one JS regex source made of letters, [..] classes and '|' (no groups in these sources). */
static void dna_parse(const char *src, DnaPattern *pat) {
  pat->nalt = 0;
  DnaAlt *cur = &pat->alt[0];
  cur->len = 0;
  pat->nalt = 1;
  for (const char *p = src; *p; p++) {
    if (*p == '|') { pat->nalt++; cur = &pat->alt[pat->nalt - 1]; cur->len = 0; continue; }
    uint8_t mask = 0;
    if (*p == '[') {
      for (p++; *p != ']'; p++) {
        if (!*p) { fprintf(stderr, "regexp-dna: bad class\n"); exit(2); }
        mask |= dna_bit(*p);
      }
    } else {
      mask = dna_bit(*p);
      if (!mask) { fprintf(stderr, "regexp-dna: unsupported source character\n"); exit(2); }
    }
    cur->m[cur->len++] = mask;
  }
}
static bool dna_alt_at(const DnaAlt *a, const char *s, size_t n, size_t pos) {
  if (pos + (size_t)a->len > n) return false;
  for (int k = 0; k < a->len; k++) {
    char ch = s[pos + k];
    if (ch >= 'A' && ch <= 'Z') ch = (char)(ch - 'A' + 'a');
    if (!(a->m[k] & dna_bit(ch))) return false;
  }
  return true;
}
static long dna_count(const DnaPattern *pat, const char *s, size_t n) {
  long count = 0;
  size_t pos = 0;
  while (pos < n) {
    bool matched = false;
    for (int k = 0; k < pat->nalt && !matched; k++) {
      if (dna_alt_at(&pat->alt[k], s, n, pos)) {
        count++;
        pos += (size_t)pat->alt[k].len;
        matched = true;
      }
    }
    if (!matched) pos++;
  }
  return count;
}
/* s.replace(key, rep) with a one-character string pattern: the first occurrence only. */
static void replace_first(Str *s, char key, const char *rep) {
  char *p = memchr(s->p, key, s->n);
  if (!p) return;
  size_t at = (size_t)(p - s->p);
  Str out = {0};
  str_add(&out, s->p, at);
  str_cat(&out, rep);
  str_add(&out, s->p + at + 1, s->n - at - 1);
  str_free(s);
  *s = out;
}
static int run_regexp_dna(void) {
  Str input = {0}, expected_output = {0}, expected_input = {0}, out = {0};
  js_literal(find_after(S.dna, "var dnaInput = \""), '"', &input, NULL);
  /* dnaInput = dnaInput + dnaInput + dnaInput */
  Str tripled = {0};
  for (int k = 0; k < 3; k++) str_add(&tripled, input.p, input.n);
  str_free(&input);
  input = tripled;
  /* dnaInput.replace(/>.*\n|\n/g, "") */
  Str stripped = {0};
  for (size_t i = 0; i < input.n; ) {
    const char *nl = input.p[i] == '>' ? memchr(input.p + i, '\n', input.n - i) : NULL;
    if (nl) {                       /* />.*\n/ : a header line up to and including its newline */
      i = (size_t)(nl - input.p) + 1;
    } else if (input.p[i] == '\n') {
      i++;
    } else {
      str_char(&stripped, input.p[i++]);
    }
  }
  str_free(&input);
  input = stripped;
  js_literal(find_after(S.dna, "var expectedDNAOutputString = \""), '"', &expected_output, NULL);
  js_literal(find_after(S.dna, "var expectedDNAInput = \""), '"', &expected_input, NULL);
  /* the counts */
  for (int k = 0; k < 9; k++) {
    DnaPattern pat;
    dna_parse(dna_sources[k], &pat);
    long n = dna_count(&pat, input.p, input.n);
    str_cat(&out, dna_sources[k]);
    str_char(&out, ' ');
    str_int(&out, n);
    str_char(&out, '\n');
  }
  /* the substitutions, in the JS object's key order */
  static const struct { char key; const char *rep; } subs[11] = {
    {'B', "(c|g|t)"}, {'D', "(a|g|t)"}, {'H', "(a|c|t)"}, {'K', "(g|t)"}, {'M', "(a|c)"}, {'N', "(a|c|g|t)"},
    {'R', "(a|g)"}, {'S', "(c|t)"}, {'V', "(a|c|g)"}, {'W', "(a|t)"}, {'Y', "(c|t)"},
  };
  for (int k = 0; k < 11; k++) replace_first(&input, subs[k].key, subs[k].rep);
  int failures = 0;
  if (out.n != expected_output.n || memcmp(out.p, expected_output.p, out.n) != 0) {
    fprintf(stderr, "regexp-dna: bad result: the match counts differ\n");
    failures++;
  }
  if (input.n != expected_input.n || memcmp(input.p, expected_input.p, input.n) != 0) {
    fprintf(stderr, "regexp-dna: bad result: the substituted sequence differs\n");
    failures++;
  }
  str_free(&input); str_free(&expected_output); str_free(&expected_input); str_free(&out);
  return failures;
}

/* ---- crypto-aes: AES-256 in the counter mode of the JS (Cipher, KeyExpansion, AESEncryptCtr,
 * AESDecryptCtr, escCtrlChars, unescCtrlChars). The S-box is read from the source; Rcon[k][0] is 0x00
 * and then the powers of two with 0x1b and 0x36 (the JS Rcon's first byte, which is all that is used).
 * The base64 and UTF-8 helpers of the file are never called by the row, so they are not ported. ---- */
static const uint8_t aes_rcon[11] = {0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36};
typedef struct { uint8_t w[60][4]; int Nr; } AesKey;
static void aes_sub_word(uint8_t *w, const uint8_t *sbox) { for (int i = 0; i < 4; i++) w[i] = sbox[w[i]]; }
static void aes_key_expansion(const uint8_t *key, int nbytes, const uint8_t *sbox, AesKey *ks) {
  int Nb = 4, Nk = nbytes / 4, Nr = Nk + 6;
  ks->Nr = Nr;
  for (int i = 0; i < Nk; i++)
    for (int t = 0; t < 4; t++) ks->w[i][t] = key[4 * i + t];
  for (int i = Nk; i < Nb * (Nr + 1); i++) {
    uint8_t temp[4];
    for (int t = 0; t < 4; t++) temp[t] = ks->w[i - 1][t];
    if (i % Nk == 0) {
      uint8_t rot[4] = {temp[1], temp[2], temp[3], temp[0]};   /* RotWord */
      for (int t = 0; t < 4; t++) temp[t] = rot[t];
      aes_sub_word(temp, sbox);
      temp[0] ^= aes_rcon[i / Nk];
    } else if (Nk > 6 && i % Nk == 4) {
      aes_sub_word(temp, sbox);
    }
    for (int t = 0; t < 4; t++) ks->w[i][t] = ks->w[i - Nk][t] ^ temp[t];
  }
}
static void aes_add_round_key(uint8_t st[4][4], const AesKey *ks, int rnd) {
  for (int r = 0; r < 4; r++)
    for (int c = 0; c < 4; c++) st[r][c] ^= ks->w[rnd * 4 + c][r];
}
static void aes_sub_bytes(uint8_t st[4][4], const uint8_t *sbox) {
  for (int r = 0; r < 4; r++)
    for (int c = 0; c < 4; c++) st[r][c] = sbox[st[r][c]];
}
static void aes_shift_rows(uint8_t st[4][4]) {
  for (int r = 1; r < 4; r++) {
    uint8_t t[4];
    for (int c = 0; c < 4; c++) t[c] = st[r][(c + r) % 4];
    for (int c = 0; c < 4; c++) st[r][c] = t[c];
  }
}
static void aes_mix_columns(uint8_t st[4][4]) {
  for (int c = 0; c < 4; c++) {
    uint8_t a[4];
    unsigned b[4];
    for (int i = 0; i < 4; i++) {
      a[i] = st[i][c];
      b[i] = (st[i][c] & 0x80) ? ((unsigned)st[i][c] << 1) ^ 0x011b : (unsigned)st[i][c] << 1;
    }
    st[0][c] = (uint8_t)(b[0] ^ a[1] ^ b[1] ^ a[2] ^ a[3]);
    st[1][c] = (uint8_t)(a[0] ^ b[1] ^ a[2] ^ b[2] ^ a[3]);
    st[2][c] = (uint8_t)(a[0] ^ a[1] ^ b[2] ^ a[3] ^ b[3]);
    st[3][c] = (uint8_t)(a[0] ^ b[0] ^ a[1] ^ a[2] ^ b[3]);
  }
}
/* Cipher(input, w): the first 16 bytes of input, the state column-major as in the JS. */
static void aes_cipher(const uint8_t *input, const AesKey *ks, const uint8_t *sbox, uint8_t *output) {
  uint8_t st[4][4];
  for (int i = 0; i < 16; i++) st[i % 4][i / 4] = input[i];
  aes_add_round_key(st, ks, 0);
  for (int round = 1; round < ks->Nr; round++) {
    aes_sub_bytes(st, sbox);
    aes_shift_rows(st);
    aes_mix_columns(st);
    aes_add_round_key(st, ks, round);
  }
  aes_sub_bytes(st, sbox);
  aes_shift_rows(st);
  aes_add_round_key(st, ks, ks->Nr);
  for (int i = 0; i < 16; i++) output[i] = st[i % 4][i / 4];
}
static void aes_read_sbox(uint8_t *sbox) {
  const char *p = find_after(S.aes, "var Sbox =  [");
  for (int i = 0; i < 256; i++) {
    char *e;
    sbox[i] = (uint8_t)strtol(p, &e, 16);
    p = e;
    while (*p == ',' || *p == ' ' || *p == '\n' || *p == '\r') p++;
  }
}
/* escCtrlChars: /[\0\t\n\v\f\r\xa0'"!-]/g replaced by '!' + code + '!' */
static bool aes_is_esc(unsigned char c) {
  return c == 0 || c == 9 || c == 10 || c == 11 || c == 12 || c == 13 || c == 160 || c == 39 || c == 34 || c == 33 || c == 45;
}
static void esc_ctrl_chars(const Str *in, Str *out) {
  out->n = 0;
  for (size_t i = 0; i < in->n; i++) {
    unsigned char c = (unsigned char)in->p[i];
    if (aes_is_esc(c)) {
      char t[16];
      int k = snprintf(t, sizeof t, "!%d!", (int)c);
      str_add(out, t, (size_t)k);
    } else {
      str_char(out, (char)c);
    }
  }
}
/* unescCtrlChars: /!\d\d?\d?!/g replaced by the character with that code */
static void unesc_ctrl_chars(const Str *in, Str *out) {
  out->n = 0;
  size_t i = 0;
  while (i < in->n) {
    if (in->p[i] == '!') {
      size_t k = 0, v = 0;
      while (k < 3 && i + 1 + k < in->n && isdigit((unsigned char)in->p[i + 1 + k])) {
        v = v * 10 + (size_t)(in->p[i + 1 + k] - '0');
        k++;
      }
      if (k > 0 && i + 1 + k < in->n && in->p[i + 1 + k] == '!') {
        str_char(out, (char)v);
        i += k + 2;
        continue;
      }
    }
    str_char(out, in->p[i++]);
  }
}
static void aes_key_from_password(const Str *password, const uint8_t *sbox, uint8_t key[32]) {
  uint8_t pw[32];
  for (int i = 0; i < 32; i++) pw[i] = (uint8_t)((size_t)i < password->n ? (unsigned char)password->p[i] : 0);
  AesKey ks0;
  aes_key_expansion(pw, 32, sbox, &ks0);
  uint8_t first[16];
  aes_cipher(pw, &ks0, sbox, first);
  for (int i = 0; i < 16; i++) { key[i] = first[i]; key[16 + i] = first[i]; }   /* key.concat(key.slice(0, 16)) */
}
static void aes_encrypt_ctr(const Str *plaintext, const Str *password, const uint8_t *sbox, Str *out) {
  uint8_t key[32];
  aes_key_from_password(password, sbox, key);
  double nonce = (double)epoch_ms();      /* new Date().getTime() */
  uint8_t counter[16];
  for (int i = 0; i < 4; i++) counter[i] = (uint8_t)(to_uint32(nonce) >> (i * 8));
  for (int i = 0; i < 4; i++) counter[i + 4] = (uint8_t)(to_uint32(nonce / 4294967296.0) >> (i * 8));
  AesKey ks;
  aes_key_expansion(key, 32, sbox, &ks);
  size_t blockCount = (plaintext->n + 15) / 16;
  Str *ciphertext = calloc(blockCount + 1, sizeof *ciphertext);
  Str ct = {0}, esc = {0};
  for (size_t b = 0; b < blockCount; b++) {
    for (int c = 0; c < 4; c++) counter[15 - c] = (uint8_t)(to_uint32((double)b) >> (c * 8));
    for (int c = 0; c < 4; c++) counter[15 - c - 4] = (uint8_t)(to_uint32((double)b / 4294967296.0) >> (c * 8));
    uint8_t cipherCntr[16];
    aes_cipher(counter, &ks, sbox, cipherCntr);
    size_t blockLength = b < blockCount - 1 ? 16 : (plaintext->n - 1) % 16 + 1;
    ct.n = 0;
    for (size_t i = 0; i < blockLength; i++) {
      unsigned char pt = (unsigned char)plaintext->p[b * 16 + i];
      str_char(&ct, (char)(pt ^ cipherCntr[i]));
    }
    esc_ctrl_chars(&ct, &ciphertext[b]);
  }
  Str ctrTxt = {0};
  for (int i = 0; i < 8; i++) str_char(&ctrTxt, (char)counter[i]);
  esc_ctrl_chars(&ctrTxt, &esc);
  out->n = 0;
  str_add(out, esc.p ? esc.p : "", esc.n);
  str_char(out, '-');
  for (size_t b = 0; b < blockCount; b++) {
    if (b) str_char(out, '-');
    str_add(out, ciphertext[b].p ? ciphertext[b].p : "", ciphertext[b].n);
  }
  for (size_t b = 0; b < blockCount; b++) str_free(&ciphertext[b]);
  free(ciphertext);
  str_free(&ct); str_free(&esc); str_free(&ctrTxt);
}
static void aes_decrypt_ctr(const Str *cipher, const Str *password, const uint8_t *sbox, Str *out) {
  uint8_t key[32];
  aes_key_from_password(password, sbox, key);
  AesKey ks;
  aes_key_expansion(key, 32, sbox, &ks);
  /* ciphertext.split('-') */
  size_t *start = malloc((cipher->n + 1) * sizeof *start), *len = malloc((cipher->n + 1) * sizeof *len);
  size_t nblocks = 0, s0 = 0;
  for (size_t i = 0; i <= cipher->n; i++) {
    if (i == cipher->n || cipher->p[i] == '-') {
      start[nblocks] = s0;
      len[nblocks] = i - s0;
      nblocks++;
      s0 = i + 1;
    }
  }
  Str blk = {0}, ctrTxt = {0};
  str_add(&blk, cipher->p + start[0], len[0]);
  unesc_ctrl_chars(&blk, &ctrTxt);
  uint8_t counter[16] = {0};
  for (int i = 0; i < 8; i++) counter[i] = (uint8_t)ctrTxt.p[i];
  out->n = 0;
  for (size_t b = 1; b < nblocks; b++) {
    for (int c = 0; c < 4; c++) counter[15 - c] = (uint8_t)(to_uint32((double)(b - 1)) >> (c * 8));
    for (int c = 0; c < 4; c++) counter[15 - c - 4] = (uint8_t)(to_uint32((double)b / 4294967296.0 - 1) >> (c * 8));
    uint8_t cipherCntr[16];
    aes_cipher(counter, &ks, sbox, cipherCntr);
    blk.n = 0;
    str_add(&blk, cipher->p + start[b], len[b]);
    Str un = {0};
    unesc_ctrl_chars(&blk, &un);
    for (size_t i = 0; i < un.n; i++) str_char(out, (char)((unsigned char)un.p[i] ^ cipherCntr[i]));
    str_free(&un);
  }
  free(start); free(len);
  str_free(&blk); str_free(&ctrTxt);
}
static int run_crypto_aes(void) {
  uint8_t sbox[256];
  aes_read_sbox(sbox);
  Str plainText = {0}, password = {0}, cipherText = {0}, decrypted = {0};
  js_literal(find_after(S.aes, "var plainText = \""), '"', &plainText, NULL);
  js_literal(find_after(S.aes, "var password = \""), '"', &password, NULL);
  aes_encrypt_ctr(&plainText, &password, sbox, &cipherText);
  aes_decrypt_ctr(&cipherText, &password, sbox, &decrypted);
  int failures = 0;
  if (decrypted.n != plainText.n || memcmp(decrypted.p, plainText.p, plainText.n) != 0) {
    fprintf(stderr, "crypto-aes: bad result: the round trip differs\n");
    failures++;
  }
  str_free(&plainText); str_free(&password); str_free(&cipherText); str_free(&decrypted);
  return failures;
}

/* ---- crypto-md5: the JS core_md5 with its words (str2binl, little-endian; holes read as 0) ---- */
static const uint32_t md5_T[64] = {
  0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
  0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
  0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
  0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
  0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
  0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
  0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
  0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
};
static uint32_t rotl32(uint32_t x, unsigned c) { return (x << c) | (x >> (32 - c)); }
static void md5_hex(const Str *msg, char hex[33]) {
  size_t len = msg->n * 8;                              /* chrsz = 8 */
  size_t nblk = ((len + 64) >> 9) + 1;
  uint32_t *x = calloc(nblk * 16, sizeof *x);
  for (size_t i = 0; i < msg->n; i++) x[i >> 2] |= (uint32_t)(unsigned char)msg->p[i] << ((i % 4) * 8);
  x[len >> 5] |= (uint32_t)0x80 << (len % 32);
  x[nblk * 16 - 2] = (uint32_t)len;
  uint32_t a = 1732584193u, b = 0xefcdab89u, c = 0x98badcfeu, d = 0x10325476u;
  static const unsigned sh[4][4] = {{7, 12, 17, 22}, {5, 9, 14, 20}, {4, 11, 16, 23}, {6, 10, 15, 21}};
  for (size_t blk = 0; blk < nblk; blk++) {
    const uint32_t *X = x + blk * 16;
    uint32_t olda = a, oldb = b, oldc = c, oldd = d;
    for (unsigned j = 0; j < 64; j++) {
      uint32_t f;
      unsigned g, r = j / 16;
      switch (r) {
        case 0: f = (b & c) | (~b & d); g = j; break;
        case 1: f = (b & d) | (c & ~d); g = (5 * j + 1) % 16; break;
        case 2: f = b ^ c ^ d; g = (3 * j + 5) % 16; break;
        default: f = c ^ (b | ~d); g = (7 * j) % 16; break;
      }
      uint32_t t = a + f + X[g] + md5_T[j];
      a = d; d = c; c = b;
      b = b + rotl32(t, sh[r][j % 4]);
    }
    a += olda; b += oldb; c += oldc; d += oldd;
  }
  free(x);
  static const char tab[] = "0123456789abcdef";
  uint32_t out[4] = {a, b, c, d};
  for (int i = 0; i < 16; i++) {
    unsigned byte = (out[i >> 2] >> ((i % 4) * 8)) & 0xff;
    hex[2 * i] = tab[byte >> 4];
    hex[2 * i + 1] = tab[byte & 15];
  }
  hex[32] = 0;
}
static int run_crypto_md5(void) {
  Str plainText = {0};
  js_literal(find_after(S.md5, "var plainText = \""), '"', &plainText, NULL);
  for (int i = 0; i < 4; i++) str_double(&plainText);
  char hex[33];
  md5_hex(&plainText, hex);
  str_free(&plainText);
  const char *expected = "a831e91e0f70eddcb70dc61c6f82f6cd";
  if (strcmp(hex, expected) != 0) {
    fprintf(stderr, "crypto-md5: bad result: expected %s but got %s\n", expected, hex);
    return 1;
  }
  return 0;
}

/* ---- crypto-sha1: core_sha1 with big-endian words ---- */
static void sha1_hex(const Str *msg, char hex[41]) {
  size_t len = msg->n * 8;
  size_t nblk = ((len + 64) >> 9) + 1;
  uint32_t *x = calloc(nblk * 16, sizeof *x);
  for (size_t i = 0; i < msg->n; i++) x[i >> 2] |= (uint32_t)(unsigned char)msg->p[i] << (24 - (i % 4) * 8);
  x[len >> 5] |= (uint32_t)0x80 << (24 - len % 32);
  x[nblk * 16 - 1] = (uint32_t)len;
  uint32_t h0 = 1732584193u, h1 = 0xefcdab89u, h2 = 0x98badcfeu, h3 = 0x10325476u, h4 = 0xc3d2e1f0u;
  uint32_t w[80];
  for (size_t blk = 0; blk < nblk; blk++) {
    const uint32_t *X = x + blk * 16;
    uint32_t a = h0, b = h1, c = h2, d = h3, e = h4;
    for (int j = 0; j < 80; j++) {
      if (j < 16) w[j] = X[j];
      else w[j] = rotl32(w[j - 3] ^ w[j - 8] ^ w[j - 14] ^ w[j - 16], 1);
      uint32_t f, k;
      if (j < 20) { f = (b & c) | (~b & d); k = 1518500249u; }
      else if (j < 40) { f = b ^ c ^ d; k = 1859775393u; }
      else if (j < 60) { f = (b & c) | (b & d) | (c & d); k = 0x8f1bbcdcu; }
      else { f = b ^ c ^ d; k = 0xca62c1d6u; }
      uint32_t t = rotl32(a, 5) + f + e + w[j] + k;
      e = d; d = c; c = rotl32(b, 30); b = a; a = t;
    }
    h0 += a; h1 += b; h2 += c; h3 += d; h4 += e;
  }
  free(x);
  static const char tab[] = "0123456789abcdef";
  uint32_t out[5] = {h0, h1, h2, h3, h4};
  for (int i = 0; i < 20; i++) {
    unsigned byte = (out[i >> 2] >> (24 - (i % 4) * 8)) & 0xff;
    hex[2 * i] = tab[byte >> 4];
    hex[2 * i + 1] = tab[byte & 15];
  }
  hex[40] = 0;
}
static int run_crypto_sha1(void) {
  Str plainText = {0};
  js_literal(find_after(S.sha1, "var plainText = \""), '"', &plainText, NULL);
  for (int i = 0; i < 4; i++) str_double(&plainText);
  char hex[41];
  sha1_hex(&plainText, hex);
  str_free(&plainText);
  const char *expected = "2524d264def74cce2498bf112bedf00e6c0b796d";
  if (strcmp(hex, expected) != 0) {
    fprintf(stderr, "crypto-sha1: bad result: expected %s but got %s\n", expected, hex);
    return 1;
  }
  return 0;
}

/* ---- date-format-tofte and date-format-xparb: Date objects as epoch milliseconds, with the local-time
 * fields from localtime_r and the local midnights from mktime (the JS Date parser's local forms). ---- */
typedef struct { int64_t ms; } JDate;
static int64_t floor_div64(int64_t a, int64_t b) { int64_t q = a / b; if ((a % b != 0) && ((a < 0) != (b < 0))) q--; return q; }
static int64_t days_from_civil(int64_t y, int64_t m, int64_t d) {          /* proleptic Gregorian, m 1..12 */
  y -= m <= 2;
  int64_t era = floor_div64(y, 400);
  int64_t yoe = y - era * 400;
  int64_t doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
  int64_t doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
  return era * 146097 + doe - 719468;
}
static struct tm jd_fields(JDate d) {
  time_t t = (time_t)floor_div64(d.ms, 1000);
  struct tm tm;
  localtime_r(&t, &tm);
  return tm;
}
/* new Date("January 1 2007 00:00:00") and the like: local time */
static JDate jd_local(int y, int mon0, int day, int h, int mi, int s) {
  struct tm tm = {0};
  tm.tm_year = y - 1900; tm.tm_mon = mon0; tm.tm_mday = day;
  tm.tm_hour = h; tm.tm_min = mi; tm.tm_sec = s; tm.tm_isdst = -1;
  JDate d = {(int64_t)mktime(&tm) * 1000};
  return d;
}
static double jd_tzoffset(JDate d) { struct tm t = jd_fields(d); return -(double)t.tm_gmtoff / 60.0; }   /* getTimezoneOffset */
static int jd_year(JDate d) { return jd_fields(d).tm_year + 1900; }
static void app_pad2(Str *o, long long v) { char t[32]; int k = snprintf(t, sizeof t, "%02lld", v); str_add(o, t, (size_t)k); }
static void app_pad(Str *o, long long v, int w) { char t[32]; int k = snprintf(t, sizeof t, "%0*lld", w, v); str_add(o, t, (size_t)k); }

/* ---- date-format-tofte: Date.prototype.formatDate(input) ---- */
static const char *tf_daysLong[7] = {"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"};
static const char *tf_daysShort[7] = {"Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"};
static const char *tf_monthsShort[12] = {"Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"};
static const char *tf_monthsLong[12] = {"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"};
/* daysSuffix: 1st, 2nd, 3rd, 21st, 22nd, 23rd, 31st; the other days are "th" */
static const char *tf_daysSuffix(int mday) {
  if (mday == 1 || mday == 21 || mday == 31) return "st";
  if (mday == 2 || mday == 22) return "nd";
  if (mday == 3 || mday == 23) return "rd";
  return "th";
}
static const char *tf_switches = "aABdDFgGhHijlLmMnOrsStUwWyYz";
static int tf_leap(JDate d) { int y = jd_year(d); return ((y % 4 == 0 && y % 100 != 0) || (y % 4 == 0 && y % 100 == 0 && y % 400 == 0)) ? 1 : 0; }
static int tf_Y(JDate d) {
  JDate newDate = {days_from_civil(2001, 1, 1) * 86400000LL};  /* new Date("January 1 2001 00:00:00 +0000") */
  if (jd_year(newDate) == 2001) return jd_year(d);
  int x = jd_fields(d).tm_year;                               /* getYear() */
  int y = x % 100;
  y += (y < 38) ? 2000 : 1900;
  return y;
}
static long tf_z(JDate d) {                                    /* day of the year, from local Jan 1 */
  JDate t = jd_local(tf_Y(d), 0, 1, 0, 0, 0);
  double diff = (double)(d.ms - t.ms);
  return (long)floor(diff / 1000 / 60 / 60 / 24);
}
static long tf_w(JDate d) { return jd_fields(d).tm_wday; }
static void tofte_format(JDate self, const char *input, Str *out);
static void tf_W(JDate self, Str *o) {
  long beforeNY = 364 + tf_leap(self) - tf_z(self);
  long afterNY = tf_z(self);
  long weekday = tf_w(self) != 0 ? tf_w(self) - 1 : 6;
  if (beforeNY <= 2 && weekday <= 2 - beforeNY) { str_cat(o, "1"); return; }
  JDate ny = jd_local(tf_Y(self), 0, 1, 0, 0, 0);
  long nyw = jd_fields(ny).tm_wday;
  long nyDay = nyw != 0 ? nyw - 1 : 6;
  if (afterNY <= 2 && nyDay >= 4 && afterNY >= 6 - nyDay) {
    JDate prevNY = jd_local(tf_Y(self) - 1, 11, 31, 0, 0, 0);
    tofte_format(prevNY, "W", o);
    return;
  }
  long v = nyDay <= 3 ? 1 + (long)floor((double)(tf_z(self) + nyDay) / 7)
                      : 1 + (long)floor((double)(tf_z(self) - (7 - nyDay)) / 7);
  str_int(o, v);
}
/* One switch character; appends the function's value. */
static void tf_call(JDate self, char c, Str *o) {
  struct tm t = jd_fields(self);
  switch (c) {
    case 'a': str_cat(o, t.tm_hour > 11 ? "pm" : "am"); break;
    case 'A': str_cat(o, t.tm_hour > 11 ? "PM" : "AM"); break;
    case 'B': {
      double off = (jd_tzoffset(self) + 60) * 60;
      double theSeconds = (t.tm_hour * 3600.0) + (t.tm_min * 60.0) + t.tm_sec + off;
      long beat = (long)floor(theSeconds / 86.4);
      if (beat > 1000) beat -= 1000;
      if (beat < 0) beat += 1000;
      app_pad(o, beat, 3);
      break;
    }
    case 'd': app_pad2(o, t.tm_mday); break;
    case 'D': str_cat(o, tf_daysShort[t.tm_wday]); break;
    case 'F': str_cat(o, tf_monthsLong[t.tm_mon]); break;
    case 'g': str_int(o, t.tm_hour > 12 ? t.tm_hour - 12 : t.tm_hour); break;
    case 'G': str_int(o, t.tm_hour); break;
    case 'h': app_pad2(o, t.tm_hour > 12 ? t.tm_hour - 12 : t.tm_hour); break;
    case 'H': app_pad2(o, t.tm_hour); break;
    case 'i': app_pad2(o, t.tm_min); break;
    case 'j': str_int(o, t.tm_mday); break;
    case 'l': str_cat(o, tf_daysLong[t.tm_wday]); break;
    case 'L': str_int(o, tf_leap(self)); break;
    case 'm': app_pad2(o, t.tm_mon + 1); break;
    case 'M': str_cat(o, tf_monthsShort[t.tm_mon]); break;
    case 'n': str_int(o, t.tm_mon + 1); break;
    case 'O': {
      long os = labs((long)jd_tzoffset(self));
      Str h = {0}, m = {0};
      app_pad2(&h, os / 60);
      app_pad2(&m, os % 60);
      str_cat(o, jd_tzoffset(self) < 0 ? "+" : "-");
      str_add(o, h.p, h.n);
      str_add(o, m.p, m.n);
      str_free(&h); str_free(&m);
      break;
    }
    case 'r': {
      tf_call(self, 'D', o); str_cat(o, ", ");
      tf_call(self, 'j', o); str_char(o, ' ');
      tf_call(self, 'M', o); str_char(o, ' ');
      tf_call(self, 'Y', o); str_char(o, ' ');
      tf_call(self, 'H', o); str_char(o, ':');
      tf_call(self, 'i', o); str_char(o, ':');
      tf_call(self, 's', o); str_char(o, ' ');
      tf_call(self, 'O', o);
      break;
    }
    case 'S': str_cat(o, tf_daysSuffix(t.tm_mday)); break;
    case 's': app_pad2(o, t.tm_sec); break;
    case 't': {
      static const int dim[13] = {0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
      if (tf_leap(self) == 1 && t.tm_mon + 1 == 2) { str_int(o, 29); break; }
      str_int(o, dim[t.tm_mon + 1]);
      break;
    }
    case 'U': str_int(o, (long long)floor((double)self.ms / 1000 + 0.5)); break;
    case 'w': str_int(o, t.tm_wday); break;
    case 'W': tf_W(self, o); break;
    case 'y': {
      Str y = {0};
      str_int(&y, tf_Y(self));
      str_add(o, y.p + y.n - 2, 2);
      str_free(&y);
      break;
    }
    case 'Y': str_int(o, tf_Y(self)); break;
    case 'z': str_int(o, tf_z(self)); break;
    default: break;
  }
}
/* formatDate(input): escapes drop the backslash and keep the next character; switch characters are
 * replaced by their values; everything else is copied. */
static void tofte_format(JDate self, const char *input, Str *out) {   /* appends to out (W recurses into it) */
  size_t n = strlen(input);
  for (size_t i = 0; i < n; ) {
    char c = input[i];
    if (c == '\\') {
      if (i + 1 < n) str_char(out, input[i + 1]);
      i += 2;
      continue;
    }
    if (strchr(tf_switches, c)) tf_call(self, c, out);
    else str_char(out, c);
    i++;
  }
}
static int run_date_format_tofte(void) {
  JDate date = jd_local(2007, 0, 1, 1, 11, 11);
  Str shortFormat = {0}, longFormat = {0};
  for (int i = 0; i < 500; ++i) {
    shortFormat.n = 0;
    longFormat.n = 0;
    tofte_format(date, "Y-m-d", &shortFormat);
    tofte_format(date, "l, F d, Y g:i:s A", &longFormat);
    date.ms += 84266956;
  }
  str_free(&shortFormat); str_free(&longFormat);
  return 0;
}

/* ---- date-format-xparb: dateFormat(format) compiles the format to the code the JS builds
 * (Date.getFormatCode) and runs it. Each character becomes its code; the codes that need document or
 * the Date.toString parse (W, T) do not occur in the row's formats and are refused. ---- */
static const char *xp_dayNames[7] = {"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"};
static const char *xp_monthNames[12] = {"January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"};
static void xparb_format(JDate self, const char *format, Str *out) {
  struct tm t = jd_fields(self);
  out->n = 0;
  bool special = false;
  for (const char *p = format; *p; p++) {
    char ch = *p;
    if (!special && ch == '\\') { special = true; continue; }
    if (special) { special = false; str_char(out, ch); continue; }
    switch (ch) {
      case 'd': app_pad2(out, t.tm_mday); break;
      case 'D': str_add(out, xp_dayNames[t.tm_wday], 3); break;
      case 'j': str_int(out, t.tm_mday); break;
      case 'l': str_cat(out, xp_dayNames[t.tm_wday]); break;
      case 'S': {
        int d = t.tm_mday;
        str_cat(out, (d == 1 || d == 21 || d == 31) ? "st" : (d == 2 || d == 22) ? "nd" : (d == 3 || d == 23) ? "rd" : "th");
        break;
      }
      case 'w': str_int(out, t.tm_wday); break;
      case 'F': str_cat(out, xp_monthNames[t.tm_mon]); break;
      case 'm': app_pad2(out, t.tm_mon + 1); break;
      case 'M': str_add(out, xp_monthNames[t.tm_mon], 3); break;
      case 'n': str_int(out, t.tm_mon + 1); break;
      case 'Y': str_int(out, t.tm_year + 1900); break;
      case 'y': { Str y = {0}; str_int(&y, t.tm_year + 1900); str_add(out, y.p + 2, 2); str_free(&y); break; }
      case 'a': str_cat(out, t.tm_hour < 12 ? "am" : "pm"); break;
      case 'A': str_cat(out, t.tm_hour < 12 ? "AM" : "PM"); break;
      case 'g': str_int(out, (t.tm_hour % 12) ? t.tm_hour % 12 : 12); break;
      case 'G': str_int(out, t.tm_hour); break;
      case 'h': app_pad2(out, (t.tm_hour % 12) ? t.tm_hour % 12 : 12); break;
      case 'H': app_pad2(out, t.tm_hour); break;
      case 'i': app_pad2(out, t.tm_min); break;
      case 's': app_pad2(out, t.tm_sec); break;
      case 'Z': str_int(out, (long long)t.tm_gmtoff); break;   /* getTimezoneOffset() * -60 */
      case 'W': case 'T': case 'O': case 'z': case 't': case 'L':
        fprintf(stderr, "date-format-xparb: format code %c is not used by this row\n", ch);
        exit(2);
      default: str_char(out, ch); break;
    }
  }
}
static int run_date_format_xparb(void) {
  JDate date = jd_local(2007, 0, 1, 1, 11, 11);
  Str shortFormat = {0}, longFormat = {0};
  for (int i = 0; i < 4000; ++i) {
    xparb_format(date, "Y-m-d", &shortFormat);
    xparb_format(date, "l, F d, Y g:i:s A", &longFormat);
    date.ms += 84266956;
  }
  str_free(&shortFormat); str_free(&longFormat);
  return 0;
}

/* ---- setup: the sources the JS preloads (the literal data is decoded from them on every run) ---- */
static void load_sources(const char *root) {
  char path[4096];
  struct { char **text; size_t *n; const char *rel; } files[] = {
    {&S.dna, &S.dna_n, "worker/bomb-subtests/regexp-dna.js"},
    {&S.unpack, &S.unpack_n, "worker/bomb-subtests/string-unpack-code.js"},
    {&S.tagcloud, &S.tagcloud_n, "worker/bomb-subtests/string-tagcloud.js"},
    {&S.md5, &S.md5_n, "worker/bomb-subtests/crypto-md5.js"},
    {&S.sha1, &S.sha1_n, "worker/bomb-subtests/crypto-sha1.js"},
    {&S.aes, &S.aes_n, "worker/bomb-subtests/crypto-aes.js"},
    {&S.fasta, &S.fasta_n, "worker/bomb-subtests/string-fasta.js"},
  };
  for (size_t k = 0; k < sizeof files / sizeof files[0]; k++) {
    snprintf(path, sizeof path, "%s/%s", root, files[k].rel);
    *files[k].text = read_file(path, files[k].n);
  }
  g_root = (char *)root;
}

typedef int (*SubtestFn)(void);
int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 80;
  const char *root = argc > 2 ? argv[2] : "quench-bench/jetstream";
  load_sources(root);
  /* the order of the JetStream driver's bomb preload list, each subtest once per iteration */
  static const struct { const char *name; SubtestFn fn; } subtests[] = {
    {"3d-raytrace", run_3d_raytrace},
    {"access-nbody", run_access_nbody},
    {"3d-morph", run_3d_morph},
    {"3d-cube", run_3d_cube},
    {"access-fannkuch", run_access_fannkuch},
    {"access-binary-trees", run_access_binary_trees},
    {"access-nsieve", run_access_nsieve},
    {"bitops-bitwise-and", run_bitops_bitwise_and},
    {"bitops-nsieve-bits", run_bitops_nsieve_bits},
    {"controlflow-recursive", run_controlflow_recursive},
    {"bitops-3bit-bits-in-byte", run_bitops_3bit},
    {"bitops-bits-in-byte", run_bitops_bits_in_byte},
    {"crypto-aes", run_crypto_aes},
    {"crypto-md5", run_crypto_md5},
    {"crypto-sha1", run_crypto_sha1},
    {"date-format-tofte", run_date_format_tofte},
    {"date-format-xparb", run_date_format_xparb},
    {"math-cordic", run_math_cordic},
    {"math-partial-sums", run_math_partial_sums},
    {"math-spectral-norm", run_math_spectral_norm},
    {"string-base64", run_string_base64},
    {"string-fasta", run_string_fasta},
    {"string-validate-input", run_string_validate_input},
    {"string-tagcloud", run_string_tagcloud},
    {"string-unpack-code", run_string_unpack_code},
    {"regexp-dna", run_regexp_dna},
  };
  const size_t nsub = sizeof subtests / sizeof subtests[0];
  if (nsub != 26) { fprintf(stderr, "subtest table has %zu entries\n", nsub); return 2; }
  int failures = 0;
  double t0 = now_ms();
  for (int it = 0; it < iterations; it++) {
    for (size_t k = 0; k < nsub; k++) {
      int f = subtests[k].fn();
      if (f) fprintf(stderr, "%s failed in iteration %d\n", subtests[k].name, it);
      failures += f;
    }
  }
  double t1 = now_ms();
  printf("bomb-workers %d iterations %.3f ms\n", iterations, t1 - t0);
  return failures ? 1 : 0;
}
