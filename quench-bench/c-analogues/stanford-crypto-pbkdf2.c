/* C analogue of JetStream's SeaMonster/stanford-crypto-pbkdf2.js: SJCL's PBKDF2 over
 * HMAC-SHA256 on big-endian 32-bit word arrays. Each iteration converts the password
 * (UTF-8) and the hex salt to words, keys an HMAC whose inner and outer hashes are
 * pre-absorbed (sjcl's `w`), runs 2048 rounds for the single 256-bit output block and
 * renders it as hex for the comparison, so a JS engine's native code is compared with
 * what a C programmer writes for the same program. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { BLOCK_WORDS = 16, DIGEST_WORDS = 8, ROUNDS = 64, WORD_BITS = 32 };
enum { MAX_BITS_WORDS = 16 };
static const uint32_t IPAD = 0x36363636, OPAD = 0x5c5c5c5c;
static const char PASSWORD[] = "b4db33f", SALT[] = "BA DB EE FB AD BE EF 00";
static const char EXPECTED[] = "5436c4c0d73e425a521f4a5a13e0fffe5cd957a823f435385fa0a75068a26803";
enum { COUNT = 2048 };

static const uint32_t INIT[DIGEST_WORDS] = {
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
static const uint32_t K[ROUNDS] = {
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};

/* A bit string as sjcl holds it: big-endian words plus a length in bits. */
typedef struct { uint32_t w[MAX_BITS_WORDS]; int bits; } Bits;

static uint32_t rotr(uint32_t x, int n) { return x >> n | x << (WORD_BITS - n); }

/* sjcl's `u`: one compression of a 16-word block, the message schedule rolled in place. */
static void block(uint32_t *h, uint32_t *w) {
  uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], hh = h[7], t, t1, t2;
  for (int i = 0; i < ROUNDS; i++) {
    if (i < BLOCK_WORDS) t = w[i];
    else {
      t1 = w[(i + 1) & 15]; t2 = w[(i + 14) & 15];
      t = w[i & 15] = (rotr(t1, 7) ^ rotr(t1, 18) ^ t1 >> 3) + (rotr(t2, 17) ^ rotr(t2, 19) ^ t2 >> 10) + w[i & 15] + w[(i + 9) & 15];
    }
    t = t + hh + (rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25)) + (g ^ (e & (f ^ g))) + K[i];
    hh = g; g = f; f = e; e = d + t; d = c; c = b; b = a;
    a = t + ((b & c) ^ (d & (b ^ c))) + (rotr(b, 2) ^ rotr(b, 13) ^ rotr(b, 22));
  }
  h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
}

/* Hash state over whole words: every update in this program is word-aligned. */
typedef struct { uint32_t h[DIGEST_WORDS], buf[BLOCK_WORDS]; int buffered; uint64_t length; } Sha256;

static void sha_reset(Sha256 *s) { memcpy(s->h, INIT, sizeof INIT); s->buffered = 0; s->length = 0; }

static void sha_update(Sha256 *s, const uint32_t *words, int n) {
  s->length += (uint64_t)n * WORD_BITS;
  for (int i = 0; i < n; i++) {
    s->buf[s->buffered++] = words[i];
    if (s->buffered == BLOCK_WORDS) { block(s->h, s->buf); s->buffered = 0; }
  }
}

static void sha_finalize(Sha256 *s, uint32_t *out) {
  uint64_t length = s->length;
  uint32_t pad[BLOCK_WORDS + 2] = {0x80000000u};
  int n = 1;
  while ((s->buffered + n + 2) % BLOCK_WORDS) n++;
  pad[n++] = (uint32_t)(length >> WORD_BITS);
  pad[n++] = (uint32_t)length;
  sha_update(s, pad, n);
  memcpy(out, s->h, sizeof s->h);
  sha_reset(s);
}

/* Keys longer than a block (hashed first by sjcl) do not occur here. */
typedef struct { Sha256 inner, outer; } Hmac;

static void hmac_init(Hmac *m, const Bits *key) {
  uint32_t pad[2][BLOCK_WORDS];
  for (int i = 0; i < BLOCK_WORDS; i++) {
    uint32_t k = i * WORD_BITS < key->bits ? key->w[i] : 0;
    pad[0][i] = k ^ IPAD;
    pad[1][i] = k ^ OPAD;
  }
  sha_reset(&m->inner); sha_update(&m->inner, pad[0], BLOCK_WORDS);
  sha_reset(&m->outer); sha_update(&m->outer, pad[1], BLOCK_WORDS);
}

static void hmac(const Hmac *m, const uint32_t *data, int n, uint32_t *out) {
  Sha256 s = m->inner;
  uint32_t inner[DIGEST_WORDS];
  sha_update(&s, data, n);
  sha_finalize(&s, inner);
  s = m->outer;
  sha_update(&s, inner, DIGEST_WORDS);
  sha_finalize(&s, out);
}

/* sjcl.codec.utf8String.toBits for an ASCII string: bytes packed big-endian. */
static Bits utf8_to_bits(const char *str) {
  Bits b = {{0}, 0};
  for (; *str; str++, b.bits += 8) b.w[b.bits / WORD_BITS] |= (uint32_t)(uint8_t)*str << (24 - b.bits % WORD_BITS);
  return b;
}

static int hex_digit(char c) { return c <= '9' ? c - '0' : (c | 0x20) - 'a' + 10; }

/* sjcl.codec.hex.toBits: drop whitespace, then four bits per digit. */
static Bits hex_to_bits(const char *str) {
  Bits b = {{0}, 0};
  for (; *str; str++) {
    if (*str == ' ') continue;
    b.w[b.bits / WORD_BITS] |= (uint32_t)hex_digit(*str) << (28 - b.bits % WORD_BITS);
    b.bits += 4;
  }
  return b;
}

static void hex_from_words(const uint32_t *w, int n, char *out) {
  for (int i = 0; i < n; i++) snprintf(out + 8 * i, 9, "%08x", w[i]);
}

/* sjcl.misc.pbkdf2 with the default length: one block, U1 ^ U2 ^ ... ^ Ucount. */
static void pbkdf2(const Bits *password, const Bits *salt, int count, uint32_t *out) {
  Hmac m;
  uint32_t msg[MAX_BITS_WORDS + 1], u[DIGEST_WORDS];
  int saltWords = salt->bits / WORD_BITS;
  hmac_init(&m, password);
  memcpy(msg, salt->w, saltWords * sizeof *msg);
  msg[saltWords] = 1;
  hmac(&m, msg, saltWords + 1, u);
  memcpy(out, u, sizeof u);
  for (int i = 1; i < count; i++) {
    hmac(&m, u, DIGEST_WORDS, u);
    for (int j = 0; j < DIGEST_WORDS; j++) out[j] ^= u[j];
  }
}

static void run(void) {
  Bits password = utf8_to_bits(PASSWORD), salt = hex_to_bits(SALT);
  uint32_t key[DIGEST_WORDS];
  char output[DIGEST_WORDS * 8 + 1];
  pbkdf2(&password, &salt, COUNT, key);
  hex_from_words(key, DIGEST_WORDS, output);
  if (strcmp(output, EXPECTED)) { fprintf(stderr, "Bad output: %s\n", output); exit(1); }
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) run();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("stanford-crypto-pbkdf2 %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
