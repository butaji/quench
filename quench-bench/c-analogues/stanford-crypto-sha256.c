/* C analogue of JetStream's SeaMonster/stanford-crypto-sha256.js: SJCL's SHA-256 over
 * big-endian 32-bit words, one fresh hash state per hash, hashing "b4d" and then its
 * 256-bit digest 4500 times per iteration and checking the final hex digest, so a JS
 * engine is compared with what a C programmer writes for the same program. SJCL
 * derives the round constants once from prime roots; C keeps them as a constant table. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

enum { BLOCK_WORDS = 16, DIGEST_WORDS = 8, ROUNDS = 64, CHAINED_HASHES = 4500 };
static const char EXPECTED[] = "719043495be84b97fe4f5d7e61c99d6d1ba0cd6974a6b10c684c25a44ddd0c03";

static const uint32_t K[ROUNDS] = {
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};
static const uint32_t INIT[DIGEST_WORDS] = {
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};

static uint32_t ror(uint32_t x, int n) { return x >> n | x << (32 - n); }

/* SJCL's block function: the message schedule is computed in place in a 16-word ring. */
static void block(uint32_t h[DIGEST_WORDS], uint32_t w[BLOCK_WORDS]) {
  uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], hh = h[7];
  for (int i = 0; i < ROUNDS; i++) {
    uint32_t t;
    if (i < BLOCK_WORDS) t = w[i];
    else {
      uint32_t x = w[(i + 1) & 15], y = w[(i + 14) & 15];
      t = w[i & 15] = (ror(x, 7) ^ ror(x, 18) ^ x >> 3) + (ror(y, 17) ^ ror(y, 19) ^ y >> 10) +
                      w[i & 15] + w[(i + 9) & 15];
    }
    t += hh + (ror(e, 6) ^ ror(e, 11) ^ ror(e, 25)) + (g ^ (e & (f ^ g))) + K[i];
    hh = g; g = f; f = e; e = d + t; d = c; c = b; b = a;
    a = t + ((b & c) ^ (d & (b ^ c))) + (ror(b, 2) ^ ror(b, 13) ^ ror(b, 22));
  }
  h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
}

/* sjcl.hash.sha256.hash over a message of whole bytes held as big-endian words. */
static void sha256(const uint8_t *msg, size_t len, uint32_t out[DIGEST_WORDS]) {
  uint32_t w[BLOCK_WORDS];
  memcpy(out, INIT, sizeof INIT);
  size_t i = 0;
  for (; i + 4 * BLOCK_WORDS <= len; i += 4 * BLOCK_WORDS) {
    for (int j = 0; j < BLOCK_WORDS; j++)
      w[j] = (uint32_t)msg[i + 4 * j] << 24 | (uint32_t)msg[i + 4 * j + 1] << 16 |
             (uint32_t)msg[i + 4 * j + 2] << 8 | msg[i + 4 * j + 3];
    block(out, w);
  }
  uint8_t tail[8 * BLOCK_WORDS] = {0};
  size_t rest = len - i, padded = rest + 9 <= 4 * BLOCK_WORDS ? 4 * BLOCK_WORDS : 8 * BLOCK_WORDS;
  memcpy(tail, msg + i, rest);
  tail[rest] = 0x80;
  uint64_t bits = (uint64_t)len * 8;
  for (int j = 0; j < 8; j++) tail[padded - 1 - j] = (uint8_t)(bits >> (8 * j));
  for (size_t k = 0; k < padded; k += 4 * BLOCK_WORDS) {
    for (int j = 0; j < BLOCK_WORDS; j++)
      w[j] = (uint32_t)tail[k + 4 * j] << 24 | (uint32_t)tail[k + 4 * j + 1] << 16 |
             (uint32_t)tail[k + 4 * j + 2] << 8 | tail[k + 4 * j + 3];
    block(out, w);
  }
}

static void words_to_bytes(const uint32_t words[DIGEST_WORDS], uint8_t bytes[4 * DIGEST_WORDS]) {
  for (int i = 0; i < DIGEST_WORDS; i++)
    for (int j = 0; j < 4; j++) bytes[4 * i + j] = (uint8_t)(words[i] >> (24 - 8 * j));
}

static void run(void) {
  uint32_t hash[DIGEST_WORDS];
  uint8_t bytes[4 * DIGEST_WORDS];
  sha256((const uint8_t *)"b4d", 3, hash);
  for (int i = 0; i < CHAINED_HASHES; i++) {
    words_to_bytes(hash, bytes);
    sha256(bytes, sizeof bytes, hash);
  }
  char hex[8 * DIGEST_WORDS + 1];
  for (int i = 0; i < DIGEST_WORDS; i++) snprintf(hex + 8 * i, 9, "%08x", hash[i]);
  if (strcmp(hex, EXPECTED) != 0) {
    fprintf(stderr, "Error: Bad result %s\n", hex);
    exit(1);
  }
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) run();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("stanford-crypto-sha256 %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  return 0;
}
