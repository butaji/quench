/* Shared pieces for the json-parse-inspector and json-stringify-inspector C analogues of
 * JetStream's SeaMonster rows. Both rows load SeaMonster/inspector-json-payload.js.z, a zlib
 * stream (RFC 1950) around a deflate stream (RFC 1951). This header provides:
 *   - a table-driven inflater, so the C side loads the same compressed bytes the JS driver loads;
 *   - a JSON DOM built in a bump arena with recursive-descent parsing and a serializer. Strings
 *     without escapes point into the source text (as a string view), strings with escapes are
 *     decoded into the arena, and each object/array's children are one contiguous block;
 *   - the ECMAScript Number::toString rule used by JSON.stringify: shortest round-trip digits
 *     by a Ryu-style exact algorithm with 128-bit power-of-five tables (no printf or strtod).
 * The DOM keeps members in insertion order, like JS objects do for non-index keys. */
#ifndef JSON_ANALOGUE_H
#define JSON_ANALOGUE_H

#include <math.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static void die(const char *msg) {
  fprintf(stderr, "%s\n", msg);
  exit(1);
}

static double now_ms(void) {
  struct timespec ts;
  timespec_get(&ts, TIME_UTC);
  return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

/* ---------- growable byte buffer ---------- */

typedef struct { char *data; size_t len, cap; } Buf;

static void buf_reserve(Buf *b, size_t extra) {
  if (b->len + extra <= b->cap) return;
  size_t cap = b->cap ? b->cap : 4096;
  while (cap < b->len + extra) cap *= 2;
  char *p = realloc(b->data, cap);
  if (!p) die("out of memory");
  b->data = p;
  b->cap = cap;
}

static inline void buf_append(Buf *b, const char *s, size_t n) {
  buf_reserve(b, n);
  memcpy(b->data + b->len, s, n);
  b->len += n;
}

static inline void buf_putc(Buf *b, char c) {
  if (b->len == b->cap) buf_reserve(b, 1);
  b->data[b->len++] = c;
}

static void buf_free(Buf *b) {
  free(b->data);
  b->data = NULL;
  b->len = b->cap = 0;
}

/* ---------- file helpers ---------- */

static unsigned char *read_file(const char *path, size_t *len) {
  FILE *f = fopen(path, "rb");
  if (!f) {
    fprintf(stderr, "cannot open %s\n", path);
    exit(1);
  }
  if (fseek(f, 0, SEEK_END) != 0) die("cannot seek");
  long sz = ftell(f);
  if (sz < 0) die("cannot size file");
  rewind(f);
  unsigned char *data = malloc((size_t)sz ? (size_t)sz : 1);
  if (!data) die("out of memory");
  if (fread(data, 1, (size_t)sz, f) != (size_t)sz) die("short read");
  fclose(f);
  *len = (size_t)sz;
  return data;
}

/* ---------- zlib / DEFLATE inflater (table-driven) ---------- */

typedef struct {
  uint16_t table[1 << 15]; /* indexed by the next `bits` input bits; entry = symbol << 4 | length */
  int bits;
} Huff;

typedef struct {
  const unsigned char *in;
  size_t inlen, pos;
  uint64_t bitbuf;
  int bitcnt;
  unsigned char *out;
  size_t outlen, outcap;
  int err;
  Huff lc, dc, ch;
} Inflate;

static const uint16_t INF_LBASE[29] = {3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27,
                                       31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258};
static const unsigned char INF_LEXT[29] = {0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2,
                                           2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0};
static const uint16_t INF_DBASE[30] = {1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129,
                                       193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097,
                                       6145, 8193, 12289, 16385, 24577};
static const unsigned char INF_DEXT[30] = {0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6,
                                           6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13};
static const unsigned char INF_ORDER[19] = {16, 17, 18, 0, 8, 7, 9, 6, 10, 5,
                                            11, 4, 12, 3, 13, 2, 14, 1, 15};

static inline void inf_need(Inflate *s, int n) {
  while (s->bitcnt < n) {
    uint64_t b = s->pos < s->inlen ? s->in[s->pos] : 0; /* zero padding; overrun is caught by the caller */
    s->pos++;
    s->bitbuf |= b << s->bitcnt;
    s->bitcnt += 8;
  }
}

static inline unsigned inf_bits(Inflate *s, int n) {
  inf_need(s, n);
  unsigned v = (unsigned)(s->bitbuf & ((1ull << n) - 1));
  s->bitbuf >>= n;
  s->bitcnt -= n;
  return v;
}

/* Builds a table from code lengths (canonical Huffman codes, bit-reversed for LSB-first input). */
static void huff_build(Huff *h, const unsigned char *lens, int n) {
  int count[16] = {0}, next[16];
  int maxb = 0;
  for (int i = 0; i < n; i++) {
    count[lens[i]]++;
    if (lens[i] > maxb) maxb = lens[i];
  }
  count[0] = 0;
  h->bits = maxb;
  if (maxb == 0) return;
  int code = 0;
  for (int b = 1; b <= 15; b++) {
    code = (code + count[b - 1]) << 1;
    next[b] = code;
  }
  unsigned size = 1u << maxb;
  for (int i = 0; i < n; i++) {
    int L = lens[i];
    if (!L) continue;
    unsigned c = (unsigned)next[L]++, r = 0;
    for (int k = 0; k < L; k++) r |= ((c >> k) & 1u) << (L - 1 - k);
    uint16_t entry = (uint16_t)((i << 4) | L);
    for (unsigned j = r; j < size; j += 1u << L) h->table[j] = entry;
  }
}

static inline int huff_decode(Inflate *s, const Huff *h) {
  inf_need(s, h->bits);
  uint16_t e = h->table[s->bitbuf & ((1u << h->bits) - 1)];
  int L = e & 15;
  if (!L) {
    s->err = 1;
    return -1;
  }
  s->bitbuf >>= L;
  s->bitcnt -= L;
  return e >> 4;
}

static int out_reserve(Inflate *s, size_t n) {
  if (s->outlen + n <= s->outcap) return 0;
  size_t cap = s->outcap ? s->outcap : (1u << 20);
  while (cap < s->outlen + n) cap *= 2;
  unsigned char *p = realloc(s->out, cap);
  if (!p) {
    s->err = 1;
    return -1;
  }
  s->out = p;
  s->outcap = cap;
  return 0;
}

static int inf_codes(Inflate *s, const Huff *lc, const Huff *dc) {
  for (;;) {
    int sym = huff_decode(s, lc);
    if (s->err) return -1;
    if (sym < 256) {
      if (out_reserve(s, 1) < 0) return -1;
      s->out[s->outlen++] = (unsigned char)sym;
    } else if (sym == 256) {
      return 0;
    } else {
      sym -= 257;
      if (sym >= 29) {
        s->err = 1;
        return -1;
      }
      unsigned len = INF_LBASE[sym] + inf_bits(s, INF_LEXT[sym]);
      int dsym = huff_decode(s, dc);
      if (s->err || dsym >= 30) {
        s->err = 1;
        return -1;
      }
      size_t dist = (size_t)INF_DBASE[dsym] + inf_bits(s, INF_DEXT[dsym]);
      if (dist > s->outlen) {
        s->err = 1;
        return -1;
      }
      if (out_reserve(s, len) < 0) return -1;
      unsigned char *o = s->out + s->outlen;
      const unsigned char *src = o - dist;
      for (unsigned i = 0; i < len; i++) o[i] = src[i]; /* overlap-safe, byte order matters */
      s->outlen += len;
    }
    if (s->pos > s->inlen + 16) { /* ran past the input: corrupt stream */
      s->err = 1;
      return -1;
    }
  }
}

static int inf_fixed(Inflate *s) {
  unsigned char lens[320];
  int i = 0;
  for (; i < 144; i++) lens[i] = 8;
  for (; i < 256; i++) lens[i] = 9;
  for (; i < 280; i++) lens[i] = 7;
  for (; i < 288; i++) lens[i] = 8;
  huff_build(&s->lc, lens, 288);
  for (i = 0; i < 30; i++) lens[i] = 5;
  huff_build(&s->dc, lens, 30);
  return inf_codes(s, &s->lc, &s->dc);
}

static int inf_dynamic(Inflate *s) {
  unsigned char lens[320], cl[19] = {0};
  int nlen = (int)inf_bits(s, 5) + 257;
  int ndist = (int)inf_bits(s, 5) + 1;
  int ncode = (int)inf_bits(s, 4) + 4;
  if (nlen > 286 || ndist > 30) {
    s->err = 1;
    return -1;
  }
  for (int i = 0; i < ncode; i++) cl[INF_ORDER[i]] = (unsigned char)inf_bits(s, 3);
  huff_build(&s->ch, cl, 19);
  memset(lens, 0, sizeof lens);
  int index = 0, total = nlen + ndist;
  while (index < total) {
    int sym = huff_decode(s, &s->ch);
    if (s->err) return -1;
    if (sym < 16) {
      lens[index++] = (unsigned char)sym;
    } else {
      unsigned char val = 0;
      int rep;
      if (sym == 16) {
        if (index == 0) {
          s->err = 1;
          return -1;
        }
        val = lens[index - 1];
        rep = 3 + (int)inf_bits(s, 2);
      } else if (sym == 17) {
        rep = 3 + (int)inf_bits(s, 3);
      } else {
        rep = 11 + (int)inf_bits(s, 7);
      }
      if (index + rep > total) {
        s->err = 1;
        return -1;
      }
      while (rep--) lens[index++] = val;
    }
  }
  if (lens[256] == 0) {
    s->err = 1;
    return -1;
  }
  huff_build(&s->lc, lens, nlen);
  huff_build(&s->dc, lens + nlen, ndist);
  return inf_codes(s, &s->lc, &s->dc);
}

static int inf_stored(Inflate *s) {
  inf_bits(s, s->bitcnt & 7); /* align to a byte boundary */
  unsigned len = inf_bits(s, 16), nlen = inf_bits(s, 16);
  if ((len ^ 0xFFFFu) != nlen) {
    s->err = 1;
    return -1;
  }
  if (out_reserve(s, len) < 0) return -1;
  for (unsigned i = 0; i < len; i++) s->out[s->outlen++] = (unsigned char)inf_bits(s, 8);
  return 0;
}

/* Inflates a zlib stream. On success *out is malloc'd and must be freed by the caller. */
static int zlib_inflate(const unsigned char *in, size_t inlen, unsigned char **out, size_t *outlen) {
  if (inlen < 6 || (in[0] & 0x0F) != 8 || (((unsigned)in[0] << 8) | in[1]) % 31 != 0) return -1;
  Inflate *s = calloc(1, sizeof *s);
  if (!s) die("out of memory");
  s->in = in + 2;
  s->inlen = inlen - 2;
  int last;
  do {
    last = (int)inf_bits(s, 1);
    unsigned type = inf_bits(s, 2);
    if (type == 0) inf_stored(s);
    else if (type == 1) inf_fixed(s);
    else if (type == 2) inf_dynamic(s);
    else s->err = 1;
  } while (!s->err && !last);
  if (s->err) {
    free(s->out);
    free(s);
    return -1;
  }
  if (!s->out) s->out = malloc(1);
  *out = s->out;
  *outlen = s->outlen;
  free(s);
  return 0;
}

/* ---------- ECMAScript Number::toString ---------- */

static const double P10[23] = {1e0,  1e1,  1e2,  1e3,  1e4,  1e5,  1e6,  1e7,  1e8,  1e9,  1e10, 1e11,
                               1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22};

/* ---------- shortest round-trip digits (Ryu-style d2d) ----------
 * For a finite double x > 0 this finds the shortest decimal that rounds back to x, and among
 * those the one closest to x (ties to even), which is the digit choice ECMA-262 Number::toString
 * requires. The interval of decimals that round to x is computed exactly with 64x128-bit products
 * against the 128-bit power-of-five tables below, so no printf or strtod is involved. The
 * asymmetric interval at powers of two (mmShift) and subnormals are handled by the same code. */

static const uint64_t DOUBLE_POW5_INV_SPLIT[342][2] = {
    {0x0000000000000001ULL, 0x2000000000000000ULL},
    {0x999999999999999aULL, 0x1999999999999999ULL},
    {0x47ae147ae147ae15ULL, 0x147ae147ae147ae1ULL},
    {0x6c8b4395810624deULL, 0x10624dd2f1a9fbe7ULL},
    {0x7a786c226809d496ULL, 0x1a36e2eb1c432ca5ULL},
    {0x61f9f01b866e43abULL, 0x14f8b588e368f084ULL},
    {0xb4c7f34938583622ULL, 0x10c6f7a0b5ed8d36ULL},
    {0x87a6520ec08d236aULL, 0x1ad7f29abcaf4857ULL},
    {0x9fb841a566d74f88ULL, 0x15798ee2308c39dfULL},
    {0xe62d01511f12a607ULL, 0x112e0be826d694b2ULL},
    {0xd6ae6881cb5109a4ULL, 0x1b7cdfd9d7bdbab7ULL},
    {0xdef1ed34a2a73aeaULL, 0x15fd7fe17964955fULL},
    {0x7f27f0f6e885c8bbULL, 0x119799812dea1119ULL},
    {0x650cb4be40d60df8ULL, 0x1c25c268497681c2ULL},
    {0xea70909833de7193ULL, 0x16849b86a12b9b01ULL},
    {0x21f3a6e0297ec143ULL, 0x1203af9ee756159bULL},
    {0x6985d7cd0f313537ULL, 0x1cd2b297d889bc2bULL},
    {0x2137dfd73f5a90f9ULL, 0x170ef54646d49689ULL},
    {0xe75fe645cc4873faULL, 0x12725dd1d243aba0ULL},
    {0xa5663d3c7a0d865dULL, 0x1d83c94fb6d2ac34ULL},
    {0x511e976394d79eb1ULL, 0x179ca10c9242235dULL},
    {0xda7edf82dd794bc1ULL, 0x12e3b40a0e9b4f7dULL},
    {0x2a6498d1625bac68ULL, 0x1e392010175ee596ULL},
    {0xeeb6e0a781e2f053ULL, 0x182db34012b25144ULL},
    {0x58924d52ce4f26a9ULL, 0x1357c299a88ea76aULL},
    {0x27507bb7b07ea441ULL, 0x1ef2d0f5da7dd8aaULL},
    {0x52a6c95fc0655034ULL, 0x18c240c4aecb13bbULL},
    {0x0eebd44c99eaa690ULL, 0x13ce9a36f23c0fc9ULL},
    {0xb17953adc3110a80ULL, 0x1fb0f6be50601941ULL},
    {0xc12ddc8b02740867ULL, 0x195a5efea6b34767ULL},
    {0x3424b06f3529a052ULL, 0x14484bfeebc29f86ULL},
    {0x901d59f290ee19dbULL, 0x1039d66589687f9eULL},
    {0x4cfbc31db4b0295fULL, 0x19f623d5a8a73297ULL},
    {0x3d9635b15d59bab2ULL, 0x14c4e977ba1f5bacULL},
    {0x97ab5e277de16228ULL, 0x109d8792fb4c4956ULL},
    {0xf2abc9d8c9689d0dULL, 0x1a95a5b7f87a0ef0ULL},
    {0x5bbca17a3aba173eULL, 0x154484932d2e725aULL},
    {0xafca1ac82efb45cbULL, 0x11039d428a8b8eaeULL},
    {0xb2dcf7a6b1920945ULL, 0x1b38fb9daa78e44aULL},
    {0xf57d92ebc141a104ULL, 0x15c72fb1552d836eULL},
    {0xc46475896767b403ULL, 0x116c262777579c58ULL},
    {0x6d6d88dbd8a5ecd2ULL, 0x1be03d0bf225c6f4ULL},
    {0x8abe071646eb23dbULL, 0x164cfda3281e38c3ULL},
    {0x6efe6c11d255b649ULL, 0x11d7314f534b609cULL},
    {0xb197134fb6ef8a0eULL, 0x1c8b821885456760ULL},
    {0x27ac0f72f8bfa1a5ULL, 0x16d601ad376ab91aULL},
    {0xb95672c260994e1eULL, 0x1244ce242c5560e1ULL},
    {0xf5571e03cdc21695ULL, 0x1d3ae36d13bbce35ULL},
    {0x2aac18030b01ababULL, 0x17624f8a762fd82bULL},
    {0xbbbce0026f348956ULL, 0x12b50c6ec4f31355ULL},
    {0x92c7ccd0b1eda889ULL, 0x1dee7a4ad4b81eefULL},
    {0xdbd30a408e57ba07ULL, 0x17f1fb6f10934bf2ULL},
    {0x7ca8d50071dfc806ULL, 0x1327fc58da0f6ff5ULL},
    {0xfaa7bb33e9660cd6ULL, 0x1ea6608e29b24cbbULL},
    {0x9552fc298784d711ULL, 0x18851a0b548ea3c9ULL},
    {0xaaa8c9bad2d0ac0eULL, 0x139dae6f76d88307ULL},
    {0xdddadc5e1e1aace3ULL, 0x1f62b0b257c0d1a5ULL},
    {0x7e48b04b4b488a4fULL, 0x191bc08eac9a4151ULL},
    {0xcb6d59d5d5d3a1d9ULL, 0x141633a556e1cddaULL},
    {0x3c577b1177dc817bULL, 0x1011c2eaabe7d7e2ULL},
    {0xc6f25e825960cf2aULL, 0x19b604aaaca62636ULL},
    {0x6bf518684780a5bbULL, 0x14919d5556eb51c5ULL},
    {0x232a79ed06008496ULL, 0x10747ddddf22a7d1ULL},
    {0xd1dd8fe1a3340756ULL, 0x1a53fc9631d10c81ULL},
    {0xa7e4731ae8f66c45ULL, 0x150ffd44f4a73d34ULL},
    {0x531d28e253f8569eULL, 0x10d9976a5d52975dULL},
    {0xeb61db03b98d5762ULL, 0x1af5bf109550f22eULL},
    {0xbc4e48cfc7a445e8ULL, 0x159165a6ddda5b58ULL},
    {0x6371d3d96c836b20ULL, 0x11411e1f17e1e2adULL},
    {0x9f1c8628ad9f11cdULL, 0x1b9b6364f3030448ULL},
    {0xe5b06b53be18db0bULL, 0x1615e91d8f359d06ULL},
    {0xeaf3890fcb4715a2ULL, 0x11ab20e472914a6bULL},
    {0x44b8db4c7871bc37ULL, 0x1c45016d841baa46ULL},
    {0x03c715d6c6c1635fULL, 0x169d9abe03495505ULL},
    {0x3638de456bcde919ULL, 0x1217aefe69077737ULL},
    {0x56c163a2461641c1ULL, 0x1cf2b1970e725858ULL},
    {0xdf011c81d1ab67ceULL, 0x17288e1271f51379ULL},
    {0x7f3416ce4155eca5ULL, 0x1286d80ec190dc61ULL},
    {0x6520247d3556476eULL, 0x1da48ce468e7c702ULL},
    {0xea801d30f7783925ULL, 0x17b6d71d20b96c01ULL},
    {0xbb99b0f3f92cfa84ULL, 0x12f8ac174d612334ULL},
    {0x5f5c4e532847f739ULL, 0x1e5aacf215683854ULL},
    {0x7f7d0b75b9d32c2eULL, 0x18488a5b44536043ULL},
    {0x9930d5f7c7dc2358ULL, 0x136d3b7c36a919cfULL},
    {0x8eb4898c72f9d226ULL, 0x1f152bf9f10e8fb2ULL},
    {0x722a07a38f2e41b8ULL, 0x18ddbcc7f40ba628ULL},
    {0xc1bb394fa5be9afaULL, 0x13e497065cd61e86ULL},
    {0x9c5ec2190930f7f6ULL, 0x1fd424d6faf030d7ULL},
    {0x49e56814075a5ff8ULL, 0x197683df2f268d79ULL},
    {0x6e51201005e1e660ULL, 0x145ecfe5bf520ac7ULL},
    {0xf1da800cd181851aULL, 0x104bd984990e6f05ULL},
    {0x4fc400148268d4f5ULL, 0x1a12f5a0f4e3e4d6ULL},
    {0xd96999aa01ed772bULL, 0x14dbf7b3f71cb711ULL},
    {0xadee1488018ac5bcULL, 0x10aff95cc5b09274ULL},
    {0x497ceda668de092cULL, 0x1ab328946f80ea54ULL},
    {0x3aca57b853e4d424ULL, 0x155c2076bf9a5510ULL},
    {0x623b7960431d7683ULL, 0x1116805effaeaa73ULL},
    {0x9d2bf566d1c8bd9eULL, 0x1b5733cb32b110b8ULL},
    {0x7dbcc452416d647fULL, 0x15df5ca28ef40d60ULL},
    {0xcafd69db678ab6ccULL, 0x117f7d4ed8c33de6ULL},
    {0xab2f0fc572778adfULL, 0x1bff2ee48e052fd7ULL},
    {0x88f273045b92d580ULL, 0x1665bf1d3e6a8cacULL},
    {0xd3f528d049424466ULL, 0x11eaff4a98553d56ULL},
    {0xb988414d4203a0a3ULL, 0x1cab3210f3bb9557ULL},
    {0x6139cdd76802e6e9ULL, 0x16ef5b40c2fc7779ULL},
    {0xe761717920025254ULL, 0x125915cd68c9f92dULL},
    {0xa568b58e999d5086ULL, 0x1d5b561574765b7cULL},
    {0x5120913ee14aa6d2ULL, 0x177c44ddf6c515fdULL},
    {0xa74d40ff1aa21f0eULL, 0x12c9d0b1923744caULL},
    {0x0baece64f769cb4aULL, 0x1e0fb44f50586e11ULL},
    {0x3c8bd850c5ee3c3bULL, 0x180c903f7379f1a7ULL},
    {0xca0979da37f1c9c9ULL, 0x133d4032c2c7f485ULL},
    {0xa9a8c2f6bfe942dbULL, 0x1ec866b79e0cba6fULL},
    {0x2153cf2bccba9be3ULL, 0x18a0522c7e709526ULL},
    {0x1aa9728970954982ULL, 0x13b374f06526ddb8ULL},
    {0xf775840f1a88759dULL, 0x1f8587e7083e2f8cULL},
    {0x5f9136727ba05e17ULL, 0x19379fec0698260aULL},
    {0x1940f85b9619e4dfULL, 0x142c7ff0054684d5ULL},
    {0xe100c6afab47ea4cULL, 0x1023998cd1053710ULL},
    {0xce67a44c453fdd47ULL, 0x19d28f47b4d524e7ULL},
    {0xd852e9d69dccb106ULL, 0x14a8729fc3ddb71fULL},
    {0x79dbee454b0a2738ULL, 0x1086c219697e2c19ULL},
    {0x295fe3a211a9d859ULL, 0x1a71368f0f30468fULL},
    {0xbab31c81a7bb137aULL, 0x15275ed8d8f36ba5ULL},
    {0x6228e39aec95a92fULL, 0x10ec4be0ad8f8951ULL},
    {0x9d0e38f7e0ef7517ULL, 0x1b13ac9aaf4c0ee8ULL},
    {0xb0d82d931a592a79ULL, 0x15a956e225d67253ULL},
    {0x8d79be0f4847552eULL, 0x11544581b7dec1dcULL},
    {0x158f967eda0bbb7cULL, 0x1bba08cf8c979c94ULL},
    {0x77a611ff14d62f97ULL, 0x162e6d72d6dfb076ULL},
    {0xf951a7ff43de8c79ULL, 0x11bebdf578b2f391ULL},
    {0xc21c3ffed2fdad8eULL, 0x1c6463225ab7ec1cULL},
    {0x01b0333242648ad8ULL, 0x16b6b5b5155ff017ULL},
    {0x0159c28e9b83a246ULL, 0x122bc490dde659acULL},
    {0xcef604175f3903a3ULL, 0x1d12d41afca3c2acULL},
    {0x725e69ac4c2d9c83ULL, 0x17424348ca1c9bbdULL},
    {0xf5185489d68ae39cULL, 0x129b69070816e2fdULL},
    {0xee8d540fbdab05c6ULL, 0x1dc574d80cf16b2fULL},
    {0xbed77672fe226b05ULL, 0x17d12a4670c1228cULL},
    {0xff12c528cb4ebc04ULL, 0x130dbb6b8d674ed6ULL},
    {0xcb513b74787df9a0ULL, 0x1e7c5f127bd87e24ULL},
    {0x090dc929f9fe614dULL, 0x18637f41fcad31b7ULL},
    {0xa0d7d42194cb810aULL, 0x1382cc34ca2427c5ULL},
    {0x67bfb9cf5478ce77ULL, 0x1f37ad21436d0c6fULL},
    {0x1fcc94a5dd2d71f9ULL, 0x18f9574dcf8a7059ULL},
    {0x7fd6dd517dbdf4c7ULL, 0x13faac3e3fa1f37aULL},
    {0xffbe2ee8c92fee0bULL, 0x1ff779fd329cb8c3ULL},
    {0x6631bf20a0f324d6ULL, 0x1992c7fdc216fa36ULL},
    {0xb827cc1a1a5c1d78ULL, 0x14756ccb01abfb5eULL},
    {0x935309ae7b7ce460ULL, 0x105df0a267bcc918ULL},
    {0x1eeb42b0c594a099ULL, 0x1a2fe76a3f9474f4ULL},
    {0xe58902270476e6e1ULL, 0x14f31f8832dd2a5cULL},
    {0xb7a0ce859d2bebe7ULL, 0x10c27fa028b0eeb0ULL},
    {0x59014a6f61dfdfd8ULL, 0x1ad0cc33744e4ab4ULL},
    {0xe0cdd525e7e64cadULL, 0x1573d68f903ea229ULL},
    {0x4d7177518651d6f1ULL, 0x11297872d9cbb4eeULL},
    {0x7be8bee8d6e957e8ULL, 0x1b758d848fac54b0ULL},
    {0xfcba3253df211320ULL, 0x15f7a46a0c89dd59ULL},
    {0x63c8284318e74280ULL, 0x1192e9ee706e4aaeULL},
    {0x060d0d3827d86a66ULL, 0x1c1e43171a4a1117ULL},
    {0x6b3da42cecad21ebULL, 0x167e9c127b6e7412ULL},
    {0x88fe1cf0bd574e56ULL, 0x11fee341fc585cdbULL},
    {0x419694b462254a23ULL, 0x1ccb0536608d615fULL},
    {0x67abaa29e81dd4e9ULL, 0x1708d0f84d3de77fULL},
    {0xb95621bb2017dd87ULL, 0x126d73f9d764b932ULL},
    {0xc223692b668c95a5ULL, 0x1d7becc2f23ac1eaULL},
    {0xce82ba891ed6de1dULL, 0x179657025b6234bbULL},
    {0xa53562074bdf1818ULL, 0x12deac01e2b4f6fcULL},
    {0x3b889cd87964f359ULL, 0x1e3113363787f194ULL},
    {0xfc6d4a46c783f5e1ULL, 0x18274291c6065adcULL},
    {0x30576e9f06032b1aULL, 0x13529ba7d19eaf17ULL},
    {0x1a257dcb3cd1de90ULL, 0x1eea92a61c311825ULL},
    {0x481dfe3c30a7e540ULL, 0x18bba884e35a79b7ULL},
    {0xd34b31c9c0865100ULL, 0x13c9539d82aec7c5ULL},
    {0x5211e942cda3b4cdULL, 0x1fa885c8d117a609ULL},
    {0x74db21023e1c90a4ULL, 0x19539e3a40dfb807ULL},
    {0xf715b401cb4a0d50ULL, 0x1442e4fb67196005ULL},
    {0xf8de299b09080aa7ULL, 0x103583fc527ab337ULL},
    {0x8e304291a80cddd7ULL, 0x19ef3993b72ab859ULL},
    {0x3e8d020e200a4b13ULL, 0x14bf6142f8eef9e1ULL},
    {0x653d9b3e80083c0fULL, 0x10991a9bfa58c7e7ULL},
    {0x6ec8f864000d2ce4ULL, 0x1a8e90f9908e0ca5ULL},
    {0x8bd3f9e999a423eaULL, 0x153eda614071a3b7ULL},
    {0x3ca994bae1501cbbULL, 0x10ff151a99f482f9ULL},
    {0xc775bac49bb3612bULL, 0x1b31bb5dc320d18eULL},
    {0xd2c4956a16291a89ULL, 0x15c162b168e70e0bULL},
    {0xdbd0778811ba7ba1ULL, 0x11678227871f3e6fULL},
    {0x2c80bf401c5d929bULL, 0x1bd8d03f3e9863e6ULL},
    {0xbd33cc3349e47549ULL, 0x16470cff6546b651ULL},
    {0xca8fd68f6e505dd4ULL, 0x11d270cc51055ea7ULL},
    {0x4419574be3b3c953ULL, 0x1c83e7ad4e6efdd9ULL},
    {0x0347790982f63aa9ULL, 0x16cfec8aa52597e1ULL},
    {0xcf6c60d468c4fbbaULL, 0x123ff06eea847980ULL},
    {0xe57a34870e07f92aULL, 0x1d331a4b10d3f59aULL},
    {0x512e906c0b399422ULL, 0x175c1508da432ae2ULL},
    {0xda8ba6bcd5c7a9b5ULL, 0x12b010d3e1cf5581ULL},
    {0x90df712e22d90f87ULL, 0x1de6815302e5559cULL},
    {0xda4c5a8b4f140c6cULL, 0x17eb9aa8cf1dde16ULL},
    {0xaea37ba2a5a9a38aULL, 0x1322e220a5b17e78ULL},
    {0x7dd25f6aa2a905a9ULL, 0x1e9e369aa2b59727ULL},
    {0x97db7f888220d154ULL, 0x187e92154ef7ac1fULL},
    {0x797c6606ce80a777ULL, 0x139874ddd8c6234cULL},
    {0x8f2d700ae4010bf1ULL, 0x1f5a549627a36badULL},
    {0x0c2459a25000d65aULL, 0x191510781fb5efbeULL},
    {0x701d1481d99a4515ULL, 0x1410d9f9b2f7f2feULL},
    {0xc017439b147b6a77ULL, 0x100d7b2e28c65bfeULL},
    {0xccf205c4ed9243f2ULL, 0x19af2b7d0e0a2ccaULL},
    {0x0a5b37d0be0e9cc2ULL, 0x148c22ca71a1bd6fULL},
    {0x0848f973cb3ee3ceULL, 0x10701bd527b4978cULL},
    {0xda0e5bec78649fb0ULL, 0x1a4cf9550c5425acULL},
    {0x7b3eaff060507fc0ULL, 0x150a6110d6a9b7bdULL},
    {0x95cbbff380406633ULL, 0x10d51a73deee2c97ULL},
    {0xefac665266cd7052ULL, 0x1aee90b964b04758ULL},
    {0x2623850eb8a459dbULL, 0x158ba6fab6f36c47ULL},
    {0x1e82d0d893b6ae49ULL, 0x113c85955f29236cULL},
    {0xfd9e1af41f8ab075ULL, 0x1b9408eefea838acULL},
    {0x97b1af29b2d559f7ULL, 0x16100725988693bdULL},
    {0xac8e25baf5777b2cULL, 0x11a66c1e139edc97ULL},
    {0x7a7d092b2258c513ULL, 0x1c3d79c9b8fe2dbfULL},
    {0x61fda0ef4ead6a76ULL, 0x169794a160cb57ccULL},
    {0xe7fe1a590bbdeec5ULL, 0x1212dd4de7091309ULL},
    {0xa6635d5b45fcb13aULL, 0x1ceafbafd80e84dcULL},
    {0x851c4aaf6b308dc8ULL, 0x172262f3133ed0b0ULL},
    {0xd0e36ef2bc26d7d4ULL, 0x1281e8c275cbda26ULL},
    {0xb49f17eac6a48c86ULL, 0x1d9ca79d894629d7ULL},
    {0x2a18dfef0550706bULL, 0x17b08617a104ee46ULL},
    {0x54e0b3259dd9f389ULL, 0x12f39e794d9d8b6bULL},
    {0x87cdeb6f62f65274ULL, 0x1e5297287c2f4578ULL},
    {0xd30b22bf825ea85dULL, 0x18421286c9bf6ac6ULL},
    {0x0f3c1bcc684bb9e4ULL, 0x13680ed23aff889fULL},
    {0x18602c7a4079296dULL, 0x1f0ce4839198da98ULL},
    {0x46b356c833942124ULL, 0x18d71d360e13e213ULL},
    {0x388f78a029434db6ULL, 0x13df4a91a4dcb4dcULL},
    {0x5a7f2766a86baf8aULL, 0x1fcbaa82a1612160ULL},
    {0x153285ebb9efbfa2ULL, 0x196fbb9bb44db44dULL},
    {0xaa8ed189618c994eULL, 0x145962e2f6a4903dULL},
    {0xeed8a7a11ad6e10cULL, 0x1047824f2bb6d9caULL},
    {0x7e27729b5e249b45ULL, 0x1a0c03b1df8af611ULL},
    {0xfe85f549181d4904ULL, 0x14d6695b193bf80dULL},
    {0xcb9e5dd4134aa0d0ULL, 0x10ab877c142ff9a4ULL},
    {0xdf63c9535211014dULL, 0x1aac0bf9b9e65c3aULL},
    {0x191ca10f74da6771ULL, 0x15566ffafb1eb02fULL},
    {0xadb080d92a4852c1ULL, 0x1111f32f2f4bc025ULL},
    {0x15e7348eaa0d5134ULL, 0x1b4feb7eb212cd09ULL},
    {0xab1f5d3eee710dc4ULL, 0x15d98932280f0a6dULL},
    {0xbc1917658b8da49dULL, 0x117ad428200c0857ULL},
    {0x2cf4f23c127c3a94ULL, 0x1bf7b9d9cce00d59ULL},
    {0xf0c3f4fcdb969543ULL, 0x165fc7e170b33de0ULL},
    {0x5a365d9716121103ULL, 0x11e6398126f5cb1aULL},
    {0x9056fc24f01ce804ULL, 0x1ca38f350b22de90ULL},
    {0xd9df301d8ce3ecd0ULL, 0x16e93f5da2824ba6ULL},
    {0xe17f59b13d8323daULL, 0x125432b14ecea2ebULL},
    {0x68cbc2b52f38395cULL, 0x1d53844ee47dd179ULL},
    {0x53d6355dbf602de3ULL, 0x177603725064a794ULL},
    {0xa9782ab165e68b1cULL, 0x12c4cf8ea6b6ec76ULL},
    {0x0f26aab56fd744faULL, 0x1e07b27dd78b13f1ULL},
    {0x3f52222abfdf6a62ULL, 0x18062864ac6f4327ULL},
    {0x65db4e88997f884eULL, 0x1338205089f29c1fULL},
    {0x6fc54a7428cc0d4aULL, 0x1ec033b40fea9365ULL},
    {0x596aa1f68709a43bULL, 0x1899c2f673220f84ULL},
    {0xadeee7f86c07b696ULL, 0x13ae3591f5b4d936ULL},
    {0x497e3ff3e00c5756ULL, 0x1f7d228322baf524ULL},
    {0xd464fff64cd6ac45ULL, 0x1930e868e89590e9ULL},
    {0x4383fff83d7889d1ULL, 0x14272053ed4473eeULL},
    {0xcf9cccc69793a174ULL, 0x101f4d0ff1038ff1ULL},
    {0x7f6147a425b90252ULL, 0x19cbae7fe805b31cULL},
    {0xcc4dd2e9b7c7350fULL, 0x14a2f1ffecd15c16ULL},
    {0x3d0b0f215fd290d9ULL, 0x10825b3323dab012ULL},
    {0x61ab4b689950e7c1ULL, 0x1a6a2b85062ab350ULL},
    {0x4e22a2ba1440b967ULL, 0x1521bc6a6b555c40ULL},
    {0x0b4ee894dd009453ULL, 0x10e7c9eebc4449cdULL},
    {0x1217da87c800ed51ULL, 0x1b0c764ac6d3a948ULL},
    {0xdb46486ca000bddaULL, 0x15a391d56bdc876cULL},
    {0x490506bd4ccd64afULL, 0x114fa7ddefe39f8aULL},
    {0xa8080ac87ae23ab1ULL, 0x1bb2a62fe638ff43ULL},
    {0x5339a239fbe82ef4ULL, 0x162884f31e93ff69ULL},
    {0x75c7b4fb2fecf25dULL, 0x11ba03f5b20fff87ULL},
    {0x22d92191e647ea2eULL, 0x1c5cd322b67fff3fULL},
    {0xb57a8141850654f2ULL, 0x16b0a8e891ffff65ULL},
    {0xc4620101373843f5ULL, 0x1226ed86db3332b7ULL},
    {0x3a366801f1f39feeULL, 0x1d0b15a491eb8459ULL},
    {0xfb5eb99b27f6198bULL, 0x173c115074bc69e0ULL},
    {0x2f7efae2865e7ad6ULL, 0x129674405d6387e7ULL},
    {0xe597f7d0d6fd9156ULL, 0x1dbd86cd6238d971ULL},
    {0x8479930d78cadaabULL, 0x17cad23de82d7ac1ULL},
    {0xd06142712d6f1556ULL, 0x1308a831868ac89aULL},
    {0x4d686a4eaf182222ULL, 0x1e74404f3daada91ULL},
    {0xa453883ef279b4e8ULL, 0x185d003f6488aedaULL},
    {0xe9dc6cff28615d87ULL, 0x137d99cc506d58aeULL},
    {0xa960ae650d6895a4ULL, 0x1f2f5c7a1a488de4ULL},
    {0xbab3beb73ded4483ULL, 0x18f2b061aea07183ULL},
    {0x2ef6322c318a9d36ULL, 0x13f559e7bee6c136ULL},
    {0xe4bd1d13827761f0ULL, 0x1feef63f97d79b89ULL},
    {0x83ca7da9352c4e5aULL, 0x198bf832dfdfafa1ULL},
    {0x9ca1fe20f756a515ULL, 0x146ff9c24cb2f2e7ULL},
    {0x4a1b31b3f9121daaULL, 0x1059949b708f28b9ULL},
    {0x435eb5ecc1b695ddULL, 0x1a28edc580e50df5ULL},
    {0x35e55e57015ede4aULL, 0x14ed8b04671da4c4ULL},
    {0xc4b77eac0118b1d5ULL, 0x10be08d0527e1d69ULL},
    {0xa12597799b5ab622ULL, 0x1ac9a7b3b7302f0fULL},
    {0x4db7ac6149155e81ULL, 0x156e1fc2f8f358d9ULL},
    {0xd7c6238107444b9bULL, 0x1124e63593f5e0adULL},
    {0x593d059b3ed3ac2bULL, 0x1b6e3d2286563449ULL},
    {0xe0fd9e15cbdc89bcULL, 0x15f1ca820511c36dULL},
    {0xb3fe18116fe3a163ULL, 0x118e3b9b37416924ULL},
    {0x866359b57fd29bd1ULL, 0x1c16c5c525357507ULL},
    {0xd1e91491330ee30eULL, 0x16789e3750f790d2ULL},
    {0x74ba76da8f3f1c0bULL, 0x11fa182c40c60d75ULL},
    {0xedf72490e531c678ULL, 0x1cc359e067a348bbULL},
    {0x8b2c1d40b75b052dULL, 0x1702ae4d1fb5d3c9ULL},
    {0x6f567dcd5f7c0424ULL, 0x12688b70e62b0fd4ULL},
    {0x7ef0c94898c66d06ULL, 0x1d74124e3d11b2edULL},
    {0x98c0a106e09ebd9fULL, 0x17900ea4fda7c257ULL},
    {0x470080d24d4bcae6ULL, 0x12d9a550caec9b79ULL},
    {0xd800ce1d487944a2ULL, 0x1e29088144adc58eULL},
    {0x1333d8176d2dd082ULL, 0x1820d39a9d57d13fULL},
    {0xa8f646792424a6ceULL, 0x134d76154aaca765ULL},
    {0x74bd3d8ea03aa47dULL, 0x1ee25688777aa56fULL},
    {0x5d64313ee6955064ULL, 0x18b51206c5fbb78cULL},
    {0x4ab68dcbebaaa6b7ULL, 0x13c40e6bd1962c70ULL},
    {0x1124161312aaa457ULL, 0x1fa01712e8f0471aULL},
    {0xda8344dc0eeee9dfULL, 0x194cdf4253f36c14ULL},
    {0xe2029d7cd8bf2180ULL, 0x143d7f6843292343ULL},
    {0x4e687dfd7a328133ULL, 0x103132b9cf541c36ULL},
    {0x4a40c9959050ceb8ULL, 0x19e851294bb9c6bdULL},
    {0x0833d477a6a70bc6ULL, 0x14b9da876fc7d231ULL},
    {0xa02976c61eec096bULL, 0x1094aed2bfd30e8dULL},
    {0x004257a364acdbdfULL, 0x1a877e1dffb81749ULL},
    {0xcd01dfb5ea23e319ULL, 0x153931b1996012a0ULL},
    {0x70ce4c91881cb5aeULL, 0x10fa8e27ade6754dULL},
    {0x1ae3adb5a69455e2ULL, 0x1b2a7d0c4970bbafULL},
    {0x7be957c4854377e8ULL, 0x15bb973d078d62f2ULL},
    {0xc987796a0435f987ULL, 0x1162df64060ab58eULL},
    {0x75a58f1006bcc271ULL, 0x1bd1656cd67788e4ULL},
    {0xf7b7a5a66bca3527ULL, 0x16411df0ab92d3e9ULL},
    {0x5fc61e1ebca1c41fULL, 0x11cdb18d560f0feeULL},
    {0xffa363646102d365ULL, 0x1c7c4f4889b1b316ULL},
    {0x32e91c504d9bdc51ULL, 0x16c9d906d48e28dfULL},
    {0x8f20e37371497d0eULL, 0x123b140576d820b2ULL},
    {0x7e9b0585820f2e7cULL, 0x1d2b533bf159cdeaULL},
    {0xcbaf379e01a5becaULL, 0x1755dc2ff447d7eeULL},
    {0x0958f94b348498a1ULL, 0x12ab168cc36cacbfULL},
};
static const uint64_t DOUBLE_POW5_SPLIT[326][2] = {
    {0x0000000000000000ULL, 0x1000000000000000ULL},
    {0x0000000000000000ULL, 0x1400000000000000ULL},
    {0x0000000000000000ULL, 0x1900000000000000ULL},
    {0x0000000000000000ULL, 0x1f40000000000000ULL},
    {0x0000000000000000ULL, 0x1388000000000000ULL},
    {0x0000000000000000ULL, 0x186a000000000000ULL},
    {0x0000000000000000ULL, 0x1e84800000000000ULL},
    {0x0000000000000000ULL, 0x1312d00000000000ULL},
    {0x0000000000000000ULL, 0x17d7840000000000ULL},
    {0x0000000000000000ULL, 0x1dcd650000000000ULL},
    {0x0000000000000000ULL, 0x12a05f2000000000ULL},
    {0x0000000000000000ULL, 0x174876e800000000ULL},
    {0x0000000000000000ULL, 0x1d1a94a200000000ULL},
    {0x0000000000000000ULL, 0x12309ce540000000ULL},
    {0x0000000000000000ULL, 0x16bcc41e90000000ULL},
    {0x0000000000000000ULL, 0x1c6bf52634000000ULL},
    {0x0000000000000000ULL, 0x11c37937e0800000ULL},
    {0x0000000000000000ULL, 0x16345785d8a00000ULL},
    {0x0000000000000000ULL, 0x1bc16d674ec80000ULL},
    {0x0000000000000000ULL, 0x1158e460913d0000ULL},
    {0x0000000000000000ULL, 0x15af1d78b58c4000ULL},
    {0x0000000000000000ULL, 0x1b1ae4d6e2ef5000ULL},
    {0x0000000000000000ULL, 0x10f0cf064dd59200ULL},
    {0x0000000000000000ULL, 0x152d02c7e14af680ULL},
    {0x0000000000000000ULL, 0x1a784379d99db420ULL},
    {0x0000000000000000ULL, 0x108b2a2c28029094ULL},
    {0x0000000000000000ULL, 0x14adf4b7320334b9ULL},
    {0x4000000000000000ULL, 0x19d971e4fe8401e7ULL},
    {0x8800000000000000ULL, 0x1027e72f1f128130ULL},
    {0xaa00000000000000ULL, 0x1431e0fae6d7217cULL},
    {0xd480000000000000ULL, 0x193e5939a08ce9dbULL},
    {0xc9a0000000000000ULL, 0x1f8def8808b02452ULL},
    {0xbe04000000000000ULL, 0x13b8b5b5056e16b3ULL},
    {0xad85000000000000ULL, 0x18a6e32246c99c60ULL},
    {0xd8e6400000000000ULL, 0x1ed09bead87c0378ULL},
    {0x878fe80000000000ULL, 0x13426172c74d822bULL},
    {0x6973e20000000000ULL, 0x1812f9cf7920e2b6ULL},
    {0x03d0da8000000000ULL, 0x1e17b84357691b64ULL},
    {0x8262889000000000ULL, 0x12ced32a16a1b11eULL},
    {0x22fb2ab400000000ULL, 0x178287f49c4a1d66ULL},
    {0xabb9f56100000000ULL, 0x1d6329f1c35ca4bfULL},
    {0xcb54395ca0000000ULL, 0x125dfa371a19e6f7ULL},
    {0xbe2947b3c8000000ULL, 0x16f578c4e0a060b5ULL},
    {0x2db399a0ba000000ULL, 0x1cb2d6f618c878e3ULL},
    {0xfc90400474400000ULL, 0x11efc659cf7d4b8dULL},
    {0x7bb4500591500000ULL, 0x166bb7f0435c9e71ULL},
    {0xdaa16406f5a40000ULL, 0x1c06a5ec5433c60dULL},
    {0xa8a4de8459868000ULL, 0x118427b3b4a05bc8ULL},
    {0xd2ce16256fe82000ULL, 0x15e531a0a1c872baULL},
    {0x87819baecbe22800ULL, 0x1b5e7e08ca3a8f69ULL},
    {0xf4b1014d3f6d5900ULL, 0x111b0ec57e6499a1ULL},
    {0x71dd41a08f48af40ULL, 0x1561d276ddfdc00aULL},
    {0x0e549208b31adb10ULL, 0x1aba4714957d300dULL},
    {0x28f4db456ff0c8eaULL, 0x10b46c6cdd6e3e08ULL},
    {0x33321216cbecfb24ULL, 0x14e1878814c9cd8aULL},
    {0xbffe969c7ee839edULL, 0x1a19e96a19fc40ecULL},
    {0xf7ff1e21cf512434ULL, 0x105031e2503da893ULL},
    {0xf5fee5aa43256d41ULL, 0x14643e5ae44d12b8ULL},
    {0x337e9f14d3eec892ULL, 0x197d4df19d605767ULL},
    {0x005e46da08ea7ab6ULL, 0x1fdca16e04b86d41ULL},
    {0xa03aec4845928cb2ULL, 0x13e9e4e4c2f34448ULL},
    {0xc849a75a56f72fdeULL, 0x18e45e1df3b0155aULL},
    {0x7a5c1130ecb4fbd6ULL, 0x1f1d75a5709c1ab1ULL},
    {0xec798abe93f11d65ULL, 0x13726987666190aeULL},
    {0xa797ed6e38ed64bfULL, 0x184f03e93ff9f4daULL},
    {0x517de8c9c728bdefULL, 0x1e62c4e38ff87211ULL},
    {0xd2eeb17e1c7976b5ULL, 0x12fdbb0e39fb474aULL},
    {0x87aa5ddda397d462ULL, 0x17bd29d1c87a191dULL},
    {0xe994f5550c7dc97bULL, 0x1dac74463a989f64ULL},
    {0x11fd195527ce9dedULL, 0x128bc8abe49f639fULL},
    {0xd67c5faa71c24568ULL, 0x172ebad6ddc73c86ULL},
    {0x8c1b77950e32d6c2ULL, 0x1cfa698c95390ba8ULL},
    {0x57912abd28dfc639ULL, 0x121c81f7dd43a749ULL},
    {0xad75756c7317b7c8ULL, 0x16a3a275d494911bULL},
    {0x98d2d2c78fdda5baULL, 0x1c4c8b1349b9b562ULL},
    {0x9f83c3bcb9ea8794ULL, 0x11afd6ec0e14115dULL},
    {0x0764b4abe8652979ULL, 0x161bcca7119915b5ULL},
    {0x493de1d6e27e73d7ULL, 0x1ba2bfd0d5ff5b22ULL},
    {0x6dc6ad264d8f0866ULL, 0x1145b7e285bf98f5ULL},
    {0xc938586fe0f2ca80ULL, 0x159725db272f7f32ULL},
    {0x7b866e8bd92f7d20ULL, 0x1afcef51f0fb5effULL},
    {0xad34051767bdae34ULL, 0x10de1593369d1b5fULL},
    {0x9881065d41ad19c1ULL, 0x15159af804446237ULL},
    {0x7ea147f492186032ULL, 0x1a5b01b605557ac5ULL},
    {0x6f24ccf8db4f3c1fULL, 0x1078e111c3556cbbULL},
    {0x4aee003712230b27ULL, 0x14971956342ac7eaULL},
    {0xdda98044d6abcdf0ULL, 0x19bcdfabc13579e4ULL},
    {0x0a89f02b062b60b6ULL, 0x10160bcb58c16c2fULL},
    {0xcd2c6c35c7b638e4ULL, 0x141b8ebe2ef1c73aULL},
    {0x8077874339a3c71dULL, 0x1922726dbaae3909ULL},
    {0xe0956914080cb8e4ULL, 0x1f6b0f092959c74bULL},
    {0x6c5d61ac8507f38eULL, 0x13a2e965b9d81c8fULL},
    {0x4774ba17a649f072ULL, 0x188ba3bf284e23b3ULL},
    {0x1951e89d8fdc6c8fULL, 0x1eae8caef261aca0ULL},
    {0x0fd3316279e9c3d9ULL, 0x132d17ed577d0be4ULL},
    {0x13c7fdbb186434cfULL, 0x17f85de8ad5c4eddULL},
    {0x58b9fd29de7d4203ULL, 0x1df67562d8b36294ULL},
    {0xb7743e3a2b0e4942ULL, 0x12ba095dc7701d9cULL},
    {0xe5514dc8b5d1db92ULL, 0x17688bb5394c2503ULL},
    {0xdea5a13ae3465277ULL, 0x1d42aea2879f2e44ULL},
    {0x0b2784c4ce0bf38aULL, 0x1249ad2594c37cebULL},
    {0xcdf165f6018ef06dULL, 0x16dc186ef9f45c25ULL},
    {0x416dbf7381f2ac88ULL, 0x1c931e8ab871732fULL},
    {0x88e497a83137abd5ULL, 0x11dbf316b346e7fdULL},
    {0xeb1dbd923d8596caULL, 0x1652efdc6018a1fcULL},
    {0x25e52cf6cce6fc7dULL, 0x1be7abd3781eca7cULL},
    {0x97af3c1a40105dceULL, 0x1170cb642b133e8dULL},
    {0xfd9b0b20d0147542ULL, 0x15ccfe3d35d80e30ULL},
    {0x3d01cde904199292ULL, 0x1b403dcc834e11bdULL},
    {0x462120b1a28ffb9bULL, 0x1108269fd210cb16ULL},
    {0xd7a968de0b33fa82ULL, 0x154a3047c694fddbULL},
    {0xcd93c3158e00f923ULL, 0x1a9cbc59b83a3d52ULL},
    {0xc07c59ed78c09bb6ULL, 0x10a1f5b813246653ULL},
    {0xb09b7068d6f0c2a3ULL, 0x14ca732617ed7fe8ULL},
    {0xdcc24c830cacf34cULL, 0x19fd0fef9de8dfe2ULL},
    {0xc9f96fd1e7ec180fULL, 0x103e29f5c2b18bedULL},
    {0x3c77cbc661e71e13ULL, 0x144db473335deee9ULL},
    {0x8b95beb7fa60e598ULL, 0x1961219000356aa3ULL},
    {0x6e7b2e65f8f91efeULL, 0x1fb969f40042c54cULL},
    {0xc50cfcffbb9bb35fULL, 0x13d3e2388029bb4fULL},
    {0xb6503c3faa82a037ULL, 0x18c8dac6a0342a23ULL},
    {0xa3e44b4f95234844ULL, 0x1efb1178484134acULL},
    {0xe66eaf11bd360d2bULL, 0x135ceaeb2d28c0ebULL},
    {0xe00a5ad62c839075ULL, 0x183425a5f872f126ULL},
    {0x980cf18bb7a47493ULL, 0x1e412f0f768fad70ULL},
    {0x5f0816f752c6c8dcULL, 0x12e8bd69aa19cc66ULL},
    {0xf6ca1cb527787b13ULL, 0x17a2ecc414a03f7fULL},
    {0xf47ca3e2715699d7ULL, 0x1d8ba7f519c84f5fULL},
    {0xf8cde66d86d62026ULL, 0x127748f9301d319bULL},
    {0xf7016008e88ba830ULL, 0x17151b377c247e02ULL},
    {0xb4c1b80b22ae923cULL, 0x1cda62055b2d9d83ULL},
    {0x50f91306f5ad1b65ULL, 0x12087d4358fc8272ULL},
    {0xe53757c8b318623fULL, 0x168a9c942f3ba30eULL},
    {0x9e852dbadfde7acfULL, 0x1c2d43b93b0a8bd2ULL},
    {0xa3133c94cbeb0cc1ULL, 0x119c4a53c4e69763ULL},
    {0x8bd80bb9fee5cff1ULL, 0x16035ce8b6203d3cULL},
    {0xaece0ea87e9f43eeULL, 0x1b843422e3a84c8bULL},
    {0x4d40c9294f238a75ULL, 0x1132a095ce492fd7ULL},
    {0x2090fb73a2ec6d12ULL, 0x157f48bb41db7bcdULL},
    {0x68b53a508ba78856ULL, 0x1adf1aea12525ac0ULL},
    {0x417144725748b536ULL, 0x10cb70d24b7378b8ULL},
    {0x51cd958eed1ae283ULL, 0x14fe4d06de5056e6ULL},
    {0xe640faf2a8619b24ULL, 0x1a3de04895e46c9fULL},
    {0xefe89cd7a93d00f7ULL, 0x1066ac2d5daec3e3ULL},
    {0xebe2c40d938c4134ULL, 0x14805738b51a74dcULL},
    {0x26db7510f86f5181ULL, 0x19a06d06e2611214ULL},
    {0x9849292a9b4592f1ULL, 0x100444244d7cab4cULL},
    {0xbe5b73754216f7adULL, 0x1405552d60dbd61fULL},
    {0xadf25052929cb598ULL, 0x1906aa78b912cba7ULL},
    {0x996ee4673743e2ffULL, 0x1f485516e7577e91ULL},
    {0xffe54ec0828a6ddfULL, 0x138d352e5096af1aULL},
    {0xbfdea270a32d0957ULL, 0x18708279e4bc5ae1ULL},
    {0x2fd64b0ccbf84badULL, 0x1e8ca3185deb719aULL},
    {0x5de5eee7ff7b2f4cULL, 0x1317e5ef3ab32700ULL},
    {0x755f6aa1ff59fb1fULL, 0x17dddf6b095ff0c0ULL},
    {0x92b7454a7f3079e7ULL, 0x1dd55745cbb7ecf0ULL},
    {0x5bb28b4e8f7e4c30ULL, 0x12a5568b9f52f416ULL},
    {0xf29f2e22335ddf3cULL, 0x174eac2e8727b11bULL},
    {0xef46f9aac035570bULL, 0x1d22573a28f19d62ULL},
    {0xd58c5c0ab8215667ULL, 0x123576845997025dULL},
    {0x4aef730d6629ac01ULL, 0x16c2d4256ffcc2f5ULL},
    {0x9dab4fd0bfb41701ULL, 0x1c73892ecbfbf3b2ULL},
    {0xa28b11e277d08e60ULL, 0x11c835bd3f7d784fULL},
    {0x8b2dd65b15c4b1f9ULL, 0x163a432c8f5cd663ULL},
    {0x6df94bf1db35de77ULL, 0x1bc8d3f7b3340bfcULL},
    {0xc4bbcf772901ab0aULL, 0x115d847ad000877dULL},
    {0x35eac354f34215cdULL, 0x15b4e5998400a95dULL},
    {0x8365742a30129b40ULL, 0x1b221effe500d3b4ULL},
    {0xd21f689a5e0ba108ULL, 0x10f5535fef208450ULL},
    {0x06a742c0f58e894aULL, 0x1532a837eae8a565ULL},
    {0x4851137132f22b9dULL, 0x1a7f5245e5a2cebeULL},
    {0xed32ac26bfd75b42ULL, 0x108f936baf85c136ULL},
    {0xa87f57306fcd3212ULL, 0x14b378469b673184ULL},
    {0xd29f2cfc8bc07e97ULL, 0x19e056584240fde5ULL},
    {0xa3a37c1dd7584f1eULL, 0x102c35f729689eafULL},
    {0x8c8c5b254d2e62e6ULL, 0x14374374f3c2c65bULL},
    {0x6faf71eea079fb9fULL, 0x1945145230b377f2ULL},
    {0x0b9b4e6a48987a87ULL, 0x1f965966bce055efULL},
    {0x674111026d5f4c94ULL, 0x13bdf7e0360c35b5ULL},
    {0xc111554308b71fbaULL, 0x18ad75d8438f4322ULL},
    {0x7155aa93cae4e7a8ULL, 0x1ed8d34e547313ebULL},
    {0x26d58a9c5ecf10c9ULL, 0x13478410f4c7ec73ULL},
    {0xf08aed437682d4fbULL, 0x1819651531f9e78fULL},
    {0xecada89454238a3aULL, 0x1e1fbe5a7e786173ULL},
    {0x73ec895cb4963664ULL, 0x12d3d6f88f0b3ce8ULL},
    {0x90e7abb3e1bbc3fdULL, 0x1788ccb6b2ce0c22ULL},
    {0x352196a0da2ab4fdULL, 0x1d6affe45f818f2bULL},
    {0x0134fe24885ab11eULL, 0x1262dfeebbb0f97bULL},
    {0xc1823dadaa715d65ULL, 0x16fb97ea6a9d37d9ULL},
    {0x31e2cd19150db4bfULL, 0x1cba7de5054485d0ULL},
    {0x1f2dc02fad2890f7ULL, 0x11f48eaf234ad3a2ULL},
    {0xa6f9303b9872b535ULL, 0x1671b25aec1d888aULL},
    {0x50b77c4a7e8f6282ULL, 0x1c0e1ef1a724eaadULL},
    {0x5272adae8f199d91ULL, 0x1188d357087712acULL},
    {0x670f591a32e004f6ULL, 0x15eb082cca94d757ULL},
    {0x40d32f60bf980633ULL, 0x1b65ca37fd3a0d2dULL},
    {0x4883fd9c77bf03e0ULL, 0x111f9e62fe44483cULL},
    {0x5aa4fd0395aec4d8ULL, 0x156785fbbdd55a4bULL},
    {0x314e3c447b1a760eULL, 0x1ac1677aad4ab0deULL},
    {0xded0e5aaccf089c9ULL, 0x10b8e0acac4eae8aULL},
    {0x96851f15802cac3bULL, 0x14e718d7d7625a2dULL},
    {0xfc2666dae037d74aULL, 0x1a20df0dcd3af0b8ULL},
    {0x9d980048cc22e68eULL, 0x10548b68a044d673ULL},
    {0x84fe005aff2ba032ULL, 0x1469ae42c8560c10ULL},
    {0xa63d8071bef6883eULL, 0x198419d37a6b8f14ULL},
    {0xcfcce08e2eb42a4eULL, 0x1fe52048590672d9ULL},
    {0x21e00c58dd309a70ULL, 0x13ef342d37a407c8ULL},
    {0x2a580f6f147cc10dULL, 0x18eb0138858d09baULL},
    {0xb4ee134ad99bf150ULL, 0x1f25c186a6f04c28ULL},
    {0x7114cc0ec80176d2ULL, 0x137798f428562f99ULL},
    {0xcd59ff127a01d486ULL, 0x18557f31326bbb7fULL},
    {0xc0b07ed7188249a8ULL, 0x1e6adefd7f06aa5fULL},
    {0xd86e4f466f516e09ULL, 0x1302cb5e6f642a7bULL},
    {0xce89e3180b25c98bULL, 0x17c37e360b3d351aULL},
    {0x822c5bde0def3beeULL, 0x1db45dc38e0c8261ULL},
    {0xf15bb96ac8b58575ULL, 0x1290ba9a38c7d17cULL},
    {0x2db2a7c57ae2e6d2ULL, 0x1734e940c6f9c5dcULL},
    {0x391f51b6d99ba086ULL, 0x1d022390f8b83753ULL},
    {0x03b3931248014454ULL, 0x1221563a9b732294ULL},
    {0x04a077d6da019569ULL, 0x16a9abc9424feb39ULL},
    {0x45c895cc9081fac3ULL, 0x1c5416bb92e3e607ULL},
    {0x8b9d5d9fda513cbaULL, 0x11b48e353bce6fc4ULL},
    {0xae84b507d0e58be8ULL, 0x1621b1c28ac20bb5ULL},
    {0x1a25e249c51eeee3ULL, 0x1baa1e332d728ea3ULL},
    {0xf057ad6e1b33554dULL, 0x114a52dffc679925ULL},
    {0x6c6d98c9a2002aa1ULL, 0x159ce797fb817f6fULL},
    {0x4788fefc0a803549ULL, 0x1b04217dfa61df4bULL},
    {0x0cb59f5d8690214eULL, 0x10e294eebc7d2b8fULL},
    {0xcfe30734e83429a1ULL, 0x151b3a2a6b9c7672ULL},
    {0x83dbc9022241340aULL, 0x1a6208b50683940fULL},
    {0xb2695da15568c086ULL, 0x107d457124123c89ULL},
    {0x1f03b509aac2f0a7ULL, 0x149c96cd6d16cbacULL},
    {0x26c4a24c1573acd1ULL, 0x19c3bc80c85c7e97ULL},
    {0x783ae56f8d684c03ULL, 0x101a55d07d39cf1eULL},
    {0x16499ecb70c25f03ULL, 0x1420eb449c8842e6ULL},
    {0x9bdc067e4cf2f6c4ULL, 0x19292615c3aa539fULL},
    {0x82d3081de02fb476ULL, 0x1f736f9b3494e887ULL},
    {0xb1c3e512ac1dd0c9ULL, 0x13a825c100dd1154ULL},
    {0xde34de57572544fcULL, 0x18922f31411455a9ULL},
    {0x55c215ed2cee963bULL, 0x1eb6bafd91596b14ULL},
    {0xb5994db43c151de5ULL, 0x133234de7ad7e2ecULL},
    {0xe2ffa1214b1a655eULL, 0x17fec216198ddba7ULL},
    {0xdbbf89699de0feb6ULL, 0x1dfe729b9ff15291ULL},
    {0x2957b5e202ac9f31ULL, 0x12bf07a143f6d39bULL},
    {0xf3ada35a8357c6feULL, 0x176ec98994f48881ULL},
    {0x70990c31242db8bdULL, 0x1d4a7bebfa31aaa2ULL},
    {0x865fa79eb69c9376ULL, 0x124e8d737c5f0aa5ULL},
    {0xe7f791866443b854ULL, 0x16e230d05b76cd4eULL},
    {0xa1f575e7fd54a669ULL, 0x1c9abd04725480a2ULL},
    {0xa53969b0fe54e801ULL, 0x11e0b622c774d065ULL},
    {0x0e87c41d3dea2202ULL, 0x1658e3ab7952047fULL},
    {0xd229b5248d64aa82ULL, 0x1bef1c9657a6859eULL},
    {0x435a1136d85eea91ULL, 0x117571ddf6c81383ULL},
    {0x143095848e76a536ULL, 0x15d2ce55747a1864ULL},
    {0x193cbae5b2144e83ULL, 0x1b4781ead1989e7dULL},
    {0x2fc5f4cf8f4cb112ULL, 0x110cb132c2ff630eULL},
    {0xbbb77203731fdd56ULL, 0x154fdd7f73bf3bd1ULL},
    {0x2aa54e844fe7d4acULL, 0x1aa3d4df50af0ac6ULL},
    {0xdaa75112b1f0e4ebULL, 0x10a6650b926d66bbULL},
    {0xd15125575e6d1e26ULL, 0x14cffe4e7708c06aULL},
    {0x85a56ead360865b0ULL, 0x1a03fde214caf085ULL},
    {0x7387652c41c53f8eULL, 0x10427ead4cfed653ULL},
    {0x50693e7752368f71ULL, 0x14531e58a03e8be8ULL},
    {0x64838e1526c4334eULL, 0x1967e5eec84e2ee2ULL},
    {0xfda4719a70754022ULL, 0x1fc1df6a7a61ba9aULL},
    {0xde86c70086494815ULL, 0x13d92ba28c7d14a0ULL},
    {0x162878c0a7db9a1aULL, 0x18cf768b2f9c59c9ULL},
    {0x5bb296f0d1d280a1ULL, 0x1f03542dfb83703bULL},
    {0x194f9e5683239064ULL, 0x1362149cbd322625ULL},
    {0x5fa385ec23ec747eULL, 0x183a99c3ec7eafaeULL},
    {0xf78c67672ce7919dULL, 0x1e494034e79e5b99ULL},
    {0x3ab7c0a07c10bb02ULL, 0x12edc82110c2f940ULL},
    {0x4965b0c89b14e9c3ULL, 0x17a93a2954f3b790ULL},
    {0x5bbf1cfac1da2433ULL, 0x1d9388b3aa30a574ULL},
    {0xb957721cb92856a0ULL, 0x127c35704a5e6768ULL},
    {0xe7ad4ea3e7726c48ULL, 0x171b42cc5cf60142ULL},
    {0xa198a24ce14f075aULL, 0x1ce2137f74338193ULL},
    {0x44ff65700cd16498ULL, 0x120d4c2fa8a030fcULL},
    {0x563f3ecc1005bdbeULL, 0x16909f3b92c83d3bULL},
    {0x2bcf0e7f14072d2eULL, 0x1c34c70a777a4c8aULL},
    {0x5b61690f6c847c3dULL, 0x11a0fc668aac6fd6ULL},
    {0xf239c35347a59b4cULL, 0x16093b802d578bcbULL},
    {0xeec83428198f021fULL, 0x1b8b8a6038ad6ebeULL},
    {0x553d20990ff96153ULL, 0x1137367c236c6537ULL},
    {0x2a8c68bf53f7b9a8ULL, 0x1585041b2c477e85ULL},
    {0x752f82ef28f5a812ULL, 0x1ae64521f7595e26ULL},
    {0x093db1d57999890bULL, 0x10cfeb353a97dad8ULL},
    {0x0b8d1e4ad7ffeb4eULL, 0x1503e602893dd18eULL},
    {0x8e7065dd8dffe622ULL, 0x1a44df832b8d45f1ULL},
    {0xf9063faa78bfefd5ULL, 0x106b0bb1fb384bb6ULL},
    {0xb747cf9516efebcaULL, 0x1485ce9e7a065ea4ULL},
    {0xe519c37a5cabe6bdULL, 0x19a742461887f64dULL},
    {0xaf301a2c79eb7036ULL, 0x1008896bcf54f9f0ULL},
    {0xdafc20b798664c43ULL, 0x140aabc6c32a386cULL},
    {0x11bb28e57e7fdf54ULL, 0x190d56b873f4c688ULL},
    {0x1629f31ede1fd72aULL, 0x1f50ac6690f1f82aULL},
    {0x4dda37f34ad3e67aULL, 0x13926bc01a973b1aULL},
    {0xe150c5f01d88e019ULL, 0x187706b0213d09e0ULL},
    {0x19a4f76c24eb181fULL, 0x1e94c85c298c4c59ULL},
    {0xb0071aa39712ef13ULL, 0x131cfd3999f7afb7ULL},
    {0x9c08e14c7cd7aad8ULL, 0x17e43c8800759ba5ULL},
    {0x030b199f9c0d958eULL, 0x1ddd4baa0093028fULL},
    {0x61e6f003c1887d79ULL, 0x12aa4f4a405be199ULL},
    {0xba60ac04b1ea9cd7ULL, 0x1754e31cd072d9ffULL},
    {0xa8f8d705de65440dULL, 0x1d2a1be4048f907fULL},
    {0xc99b8663aaff4a88ULL, 0x123a516e82d9ba4fULL},
    {0xbc0267fc95bf1d2aULL, 0x16c8e5ca239028e3ULL},
    {0xab0301fbbb2ee474ULL, 0x1c7b1f3cac74331cULL},
    {0xeae1e13d54fd4ec9ULL, 0x11ccf385ebc89ff1ULL},
    {0x659a598caa3ca27bULL, 0x1640306766bac7eeULL},
    {0xff00efefd4cbcb1aULL, 0x1bd03c81406979e9ULL},
    {0x3f6095f5e4ff5ef0ULL, 0x116225d0c841ec32ULL},
    {0xcf38bb735e3f36acULL, 0x15baaf44fa52673eULL},
    {0x8306ea5035cf0457ULL, 0x1b295b1638e7010eULL},
    {0x11e4527221a162b6ULL, 0x10f9d8ede39060a9ULL},
    {0x565d670eaa09bb64ULL, 0x15384f295c7478d3ULL},
    {0x2bf4c0d2548c2a3dULL, 0x1a8662f3b3919708ULL},
    {0x1b78f88374d79a66ULL, 0x1093fdd8503afe65ULL},
    {0x625736a4520d8100ULL, 0x14b8fd4e6449bdfeULL},
    {0xfaed044d6690e140ULL, 0x19e73ca1fd5c2d7dULL},
    {0xbcd422b0601a8cc8ULL, 0x103085e53e599c6eULL},
    {0x6c092b5c78212ffaULL, 0x143ca75e8df0038aULL},
    {0x070b763396297bf8ULL, 0x194bd136316c046dULL},
    {0x48ce53c07bb3daf6ULL, 0x1f9ec583bdc70588ULL},
    {0x2d80f4584d5068daULL, 0x13c33b72569c6375ULL},
    {0x78e1316e60a48310ULL, 0x18b40a4eec437c52ULL},
};
static inline uint32_t pow5bits(int32_t e) { return (uint32_t)(((e * 1217359) >> 19) + 1); }
static inline uint32_t log10Pow2(int32_t e) { return (uint32_t)((e * 78913) >> 18); }
static inline uint32_t log10Pow5(int32_t e) { return (uint32_t)((e * 732923) >> 20); }

static inline uint32_t pow5Factor(uint64_t v) {
  uint32_t n = 0;
  for (;;) {
    uint64_t q = v / 5;
    if (v - 5 * q != 0) break;
    v = q;
    n++;
  }
  return n;
}
static inline int multipleOfPowerOf5(uint64_t v, uint32_t p) { return pow5Factor(v) >= p; }
static inline int multipleOfPowerOf2(uint64_t v, uint32_t p) { return (v & ((1ull << p) - 1)) == 0; }

/* floor((m * mul) / 2^j) for a 128-bit mul = mul[1] * 2^64 + mul[0], with 64 <= j < 192. */
static inline uint64_t mulShift64(uint64_t m, const uint64_t mul[2], int32_t j) {
  unsigned __int128 b0 = (unsigned __int128)m * mul[0];
  unsigned __int128 b2 = (unsigned __int128)m * mul[1];
  return (uint64_t)(((b0 >> 64) + b2) >> (j - 64));
}

static inline uint64_t mulShiftAll64(uint64_t m, const uint64_t mul[2], int32_t j, uint64_t *vp, uint64_t *vm,
                                     uint32_t mmShift) {
  *vp = mulShift64(4 * m + 2, mul, j);
  *vm = mulShift64(4 * m - 1 - mmShift, mul, j);
  return mulShift64(4 * m, mul, j);
}

/* Writes output * 10^exp10 as the shortest decimal in the rounding interval of x. */
static void shortest_roundtrip(double x, uint64_t *output, int32_t *exp10) {
  uint64_t bits;
  memcpy(&bits, &x, sizeof bits);
  uint64_t ieeeMantissa = bits & ((1ull << 52) - 1);
  uint32_t ieeeExponent = (uint32_t)((bits >> 52) & 0x7FF);
  int32_t e2;
  uint64_t m2;
  if (ieeeExponent == 0) {
    e2 = 1 - 1023 - 52 - 2;
    m2 = ieeeMantissa;
  } else {
    e2 = (int32_t)ieeeExponent - 1023 - 52 - 2;
    m2 = (1ull << 52) | ieeeMantissa;
  }
  int acceptBounds = (m2 & 1) == 0;
  uint64_t mv = 4 * m2;
  uint32_t mmShift = ieeeMantissa != 0 || ieeeExponent <= 1;

  uint64_t vr, vp, vm;
  int32_t e10;
  int vmIsTrailingZeros = 0, vrIsTrailingZeros = 0;
  if (e2 >= 0) {
    uint32_t q = log10Pow2(e2) - (e2 > 3);
    e10 = (int32_t)q;
    int32_t k = 125 + (int32_t)pow5bits((int32_t)q) - 1;
    int32_t i = -e2 + (int32_t)q + k;
    vr = mulShiftAll64(m2, DOUBLE_POW5_INV_SPLIT[q], i, &vp, &vm, mmShift);
    if (q <= 21) {
      uint32_t mvMod5 = (uint32_t)(mv % 5);
      if (mvMod5 == 0) {
        vrIsTrailingZeros = multipleOfPowerOf5(mv, q);
      } else if (acceptBounds) {
        vmIsTrailingZeros = multipleOfPowerOf5(mv - 1 - mmShift, q);
      } else {
        vp -= multipleOfPowerOf5(mv + 2, q);
      }
    }
  } else {
    uint32_t q = log10Pow5(-e2) - (-e2 > 1);
    e10 = (int32_t)q + e2;
    int32_t i = -e2 - (int32_t)q;
    int32_t k = (int32_t)pow5bits(i) - 125;
    int32_t j = (int32_t)q - k;
    vr = mulShiftAll64(m2, DOUBLE_POW5_SPLIT[i], j, &vp, &vm, mmShift);
    if (q <= 1) {
      vrIsTrailingZeros = 1;
      if (acceptBounds) {
        vmIsTrailingZeros = mmShift == 1;
      } else {
        --vp;
      }
    } else if (q < 63) {
      vrIsTrailingZeros = multipleOfPowerOf2(mv, q - 1);
    }
  }

  uint32_t lastRemovedDigit = 0;
  if (vmIsTrailingZeros || vrIsTrailingZeros) {
    /* rare general case: trailing zeros in the interval need exact tracking */
    for (;;) {
      uint64_t vpDiv10 = vp / 10, vmDiv10 = vm / 10;
      if (vpDiv10 <= vmDiv10) break;
      uint32_t vmMod10 = (uint32_t)(vm - 10 * vmDiv10);
      uint64_t vrDiv10 = vr / 10;
      uint32_t vrMod10 = (uint32_t)(vr - 10 * vrDiv10);
      vmIsTrailingZeros &= vmMod10 == 0;
      vrIsTrailingZeros &= lastRemovedDigit == 0;
      lastRemovedDigit = vrMod10;
      vr = vrDiv10;
      vp = vpDiv10;
      vm = vmDiv10;
      ++e10;
    }
    if (vmIsTrailingZeros) {
      for (;;) {
        uint64_t vmDiv10 = vm / 10;
        uint32_t vmMod10 = (uint32_t)(vm - 10 * vmDiv10);
        if (vmMod10 != 0) break;
        uint64_t vpDiv10 = vp / 10, vrDiv10 = vr / 10;
        uint32_t vrMod10 = (uint32_t)(vr - 10 * vrDiv10);
        vrIsTrailingZeros &= lastRemovedDigit == 0;
        lastRemovedDigit = vrMod10;
        vr = vrDiv10;
        vp = vpDiv10;
        vm = vmDiv10;
        ++e10;
      }
    }
    if (vrIsTrailingZeros && lastRemovedDigit == 5 && vr % 2 == 0) lastRemovedDigit = 4; /* round to even */
    *output = vr + ((vr == vm && (!acceptBounds || !vmIsTrailingZeros)) || lastRemovedDigit >= 5);
  } else {
    /* common case: drop two digits at a time while the interval still holds a shorter decimal */
    int roundUp = 0;
    uint64_t vpDiv100 = vp / 100, vmDiv100 = vm / 100;
    if (vpDiv100 > vmDiv100) {
      uint64_t vrDiv100 = vr / 100;
      uint32_t vrMod100 = (uint32_t)(vr - 100 * vrDiv100);
      roundUp = vrMod100 >= 50;
      vr = vrDiv100;
      vp = vpDiv100;
      vm = vmDiv100;
      e10 += 2;
    }
    for (;;) {
      uint64_t vpDiv10 = vp / 10, vmDiv10 = vm / 10;
      if (vpDiv10 <= vmDiv10) break;
      uint64_t vrDiv10 = vr / 10;
      uint32_t vrMod10 = (uint32_t)(vr - 10 * vrDiv10);
      roundUp = vrMod10 >= 5;
      vr = vrDiv10;
      vp = vpDiv10;
      vm = vmDiv10;
      ++e10;
    }
    *output = vr + (vr == vm || roundUp);
  }
  *exp10 = e10;
}

/* end of shortest round-trip section */
static size_t fmt_u64(uint64_t v, char *o) {
  char t[20];
  int k = 0;
  do {
    t[k++] = (char)('0' + v % 10);
    v /= 10;
  } while (v);
  for (int i = 0; i < k; i++) o[i] = t[k - 1 - i];
  return (size_t)k;
}

/* Writes the JS string form of x (at least 32 bytes in out) and returns its length. */
static size_t js_number_to_string(double x, char *out) {
  if (x != x) {
    memcpy(out, "NaN", 3);
    return 3;
  }
  if (isinf(x)) {
    if (x < 0) {
      memcpy(out, "-Infinity", 9);
      return 9;
    }
    memcpy(out, "Infinity", 8);
    return 8;
  }
  if (x == 0) {
    out[0] = '0';
    return 1;
  }
  char *p = out;
  if (x < 0) {
    *p++ = '-';
    x = -x;
  }
  if (x < 9007199254740992.0 && x == floor(x)) return (size_t)(p - out) + fmt_u64((uint64_t)x, p);
  uint64_t sig;
  int32_t e10;
  shortest_roundtrip(x, &sig, &e10);
  char digits[24], t[24];
  int L = 0;
  for (uint64_t v = sig; v; v /= 10) t[L++] = (char)('0' + v % 10);
  for (int i = 0; i < L; i++) digits[i] = t[L - 1 - i];
  int k = L;
  while (k > 1 && digits[k - 1] == '0') k--;
  int n = L + e10;
  if (k <= n && n <= 21) {
    memcpy(p, digits, (size_t)k);
    p += k;
    for (int i = 0; i < n - k; i++) *p++ = '0';
  } else if (0 < n && n <= 21) {
    memcpy(p, digits, (size_t)n);
    p += n;
    *p++ = '.';
    memcpy(p, digits + n, (size_t)(k - n));
    p += k - n;
  } else if (-6 < n && n <= 0) {
    *p++ = '0';
    *p++ = '.';
    for (int i = 0; i < -n; i++) *p++ = '0';
    memcpy(p, digits, (size_t)k);
    p += k;
  } else {
    *p++ = digits[0];
    if (k > 1) {
      *p++ = '.';
      memcpy(p, digits + 1, (size_t)(k - 1));
      p += k - 1;
    }
    *p++ = 'e';
    int ee = n - 1;
    *p++ = ee < 0 ? '-' : '+';
    p += fmt_u64((uint64_t)(ee < 0 ? -ee : ee), p);
  }
  return (size_t)(p - out);
}

/* ---------- arena ---------- */

typedef struct Chunk { struct Chunk *next; size_t cap, used; } Chunk; /* data follows the header */

typedef struct { Chunk *first, *cur; } Arena;

static Chunk *chunk_new(size_t cap) {
  Chunk *c = malloc(sizeof *c + cap);
  if (!c) die("out of memory");
  c->next = NULL;
  c->cap = cap;
  c->used = 0;
  return c;
}

static inline void *arena_alloc(Arena *a, size_t n) {
  n = (n + 7) & ~(size_t)7;
  if (!a->cur) {
    a->first = a->cur = chunk_new(n > (1u << 20) ? n : (1u << 20));
  } else if (a->cur->used + n > a->cur->cap) {
    Chunk *c = a->cur->next;
    if (!c || c->cap < n) {
      Chunk *big = chunk_new(n > (1u << 20) ? n : (1u << 20));
      big->next = c ? c->next : NULL;
      if (c) free(c);
      c = big;
      a->cur->next = c;
    }
    c->used = 0;
    a->cur = c;
  }
  void *p = (char *)(a->cur + 1) + a->cur->used;
  a->cur->used += n;
  return p;
}

static void arena_reset(Arena *a) {
  if (a->first) a->first->used = 0;
  a->cur = a->first;
}

static void arena_free(Arena *a) {
  for (Chunk *c = a->first; c;) {
    Chunk *next = c->next;
    free(c);
    c = next;
  }
  a->first = a->cur = NULL;
}

/* ---------- JSON DOM ---------- */

typedef enum { J_NULL, J_FALSE, J_TRUE, J_NUMBER, J_STRING, J_ARRAY, J_OBJECT } JType;
typedef struct JValue JValue;
typedef struct { const char *key; uint32_t klen; JValue *value; } JMember;
struct JValue {
  uint8_t type;
  uint32_t count; /* J_ARRAY / J_OBJECT: number of children */
  union {
    double number;
    struct { const char *p; size_t n; } str;
    JValue **items;   /* J_ARRAY elements, contiguous in the arena */
    JMember *members; /* J_OBJECT members in insertion order, contiguous in the arena */
  } u;
};

typedef struct { size_t objects, arrays, strings, numbers; } JCounts;

/* A document: its arena plus the child stacks used while parsing (so children are copied once
 * into a contiguous arena block). src keeps an owned source buffer alive for the DOM's strings. */
typedef struct {
  Arena arena;
  JValue **istk;
  size_t isp, icap;
  JMember *mstk;
  size_t msp, mcap;
  unsigned char *src;
} JDoc;

static JDoc *doc_new(void) {
  JDoc *d = calloc(1, sizeof *d);
  if (!d) die("out of memory");
  return d;
}

/* Frees the DOM's memory before reuse; the next parse starts from an empty arena. */
__attribute__((unused)) static void doc_reset(JDoc *d) {
  arena_reset(&d->arena);
  d->isp = d->msp = 0;
}

static void doc_free(JDoc *d) {
  arena_free(&d->arena);
  free(d->istk);
  free(d->mstk);
  free(d->src);
  free(d);
}

static JValue *jv_node(JDoc *d, JType t) {
  JValue *v = arena_alloc(&d->arena, sizeof *v);
  v->type = (uint8_t)t;
  v->count = 0;
  return v;
}

/* Word-at-a-time scan: returns the first byte at or after q that is a quote, a backslash, a
 * control byte, or (when with_ed) 0xED, or e. Reads 8 bytes at a time (little-endian, as the
 * lowest flagged byte is the first match); the tail is scanned bytewise. */
static inline const unsigned char *json_scan(const unsigned char *q, const unsigned char *e, int with_ed) {
  const uint64_t ones = 0x0101010101010101ull, highs = 0x8080808080808080ull;
  while (e - q >= 8) {
    uint64_t w;
    memcpy(&w, q, 8);
    uint64_t dq = w ^ (ones * '"'), bs = w ^ (ones * '\\'), ed = w ^ (ones * 0xED);
    uint64_t m = ((w - ones * 0x20) & ~w & highs) | ((dq - ones) & ~dq & highs) | ((bs - ones) & ~bs & highs);
    if (with_ed) m |= (ed - ones) & ~ed & highs;
    if (m) return q + (__builtin_ctzll(m) >> 3);
    q += 8;
  }
  while (q < e && *q != '"' && *q != '\\' && *q >= 0x20 && !(with_ed && *q == 0xED)) q++;
  return q;
}

/* ---------- parser ---------- */

typedef struct {
  const unsigned char *p, *end;
  JDoc *d;
  JCounts *counts;
} JParser;

static JValue *jp_value(JParser *j);

static inline void jp_skip(JParser *j) {
  while (j->p < j->end && (*j->p == ' ' || *j->p == '\t' || *j->p == '\n' || *j->p == '\r')) j->p++;
}

static int hex4(const unsigned char *p, uint32_t *cp) {
  uint32_t v = 0;
  for (int i = 0; i < 4; i++) {
    unsigned c = p[i], d;
    if (c >= '0' && c <= '9') d = c - '0';
    else if (c >= 'a' && c <= 'f') d = c - 'a' + 10;
    else if (c >= 'A' && c <= 'F') d = c - 'A' + 10;
    else return 0;
    v = v * 16 + d;
  }
  *cp = v;
  return 1;
}

/* Encodes code point cp as UTF-8 into o; lone surrogates are kept as 3-byte sequences (WTF-8). */
static size_t utf8_encode(char *o, uint32_t cp) {
  if (cp < 0x80) {
    o[0] = (char)cp;
    return 1;
  }
  if (cp < 0x800) {
    o[0] = (char)(0xC0 | (cp >> 6));
    o[1] = (char)(0x80 | (cp & 0x3F));
    return 2;
  }
  if (cp < 0x10000) {
    o[0] = (char)(0xE0 | (cp >> 12));
    o[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
    o[2] = (char)(0x80 | (cp & 0x3F));
    return 3;
  }
  o[0] = (char)(0xF0 | (cp >> 18));
  o[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
  o[2] = (char)(0x80 | ((cp >> 6) & 0x3F));
  o[3] = (char)(0x80 | (cp & 0x3F));
  return 4;
}

/* Parses the string literal at j->p (which is '"'). Without escapes the result points into the
 * source text; with escapes the decoded bytes are copied into the arena. */
static int jp_string(JParser *j, const char **out, size_t *outlen) {
  const unsigned char *s = j->p + 1, *e = j->end;
  const unsigned char *q = json_scan(s, e, 0);
  if (q >= e) return 0;
  if (*q == '"') {
    *out = (const char *)s;
    *outlen = (size_t)(q - s);
    j->p = q + 1;
    return 1;
  }
  /* slow path: find the closing quote, then decode */
  const unsigned char *r = q;
  while (r < e && *r != '"') {
    if (*r == '\\') {
      r++;
      if (r >= e) return 0;
    } else if (*r < 0x20) {
      return 0;
    }
    r++;
  }
  if (r >= e) return 0;
  size_t span = (size_t)(r - s);
  char *buf = arena_alloc(&j->d->arena, span ? span : 1);
  size_t n = 0;
  const unsigned char *p = s;
  while (p < r) {
    if (*p != '\\') {
      buf[n++] = (char)*p++;
      continue;
    }
    unsigned c = p[1];
    p += 2;
    switch (c) {
    case '"': case '\\': case '/': buf[n++] = (char)c; break;
    case 'b': buf[n++] = '\b'; break;
    case 'f': buf[n++] = '\f'; break;
    case 'n': buf[n++] = '\n'; break;
    case 'r': buf[n++] = '\r'; break;
    case 't': buf[n++] = '\t'; break;
    case 'u': {
      uint32_t cp;
      if (r - p < 4 || !hex4(p, &cp)) return 0;
      p += 4;
      if (cp >= 0xD800 && cp <= 0xDBFF && r - p >= 6 && p[0] == '\\' && p[1] == 'u') {
        uint32_t lo;
        if (hex4(p + 2, &lo) && lo >= 0xDC00 && lo <= 0xDFFF) {
          cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
          p += 6;
        }
      }
      n += utf8_encode(buf + n, cp);
      break;
    }
    default: return 0;
    }
  }
  *out = buf;
  *outlen = n;
  j->p = r + 1;
  return 1;
}

static JValue *jp_number(JParser *j) {
  const unsigned char *s = j->p, *p = s, *e = j->end;
  int neg = 0;
  if (p < e && *p == '-') {
    neg = 1;
    p++;
  }
  if (p < e && *p == '0') {
    p++;
  } else if (p < e && *p >= '1' && *p <= '9') {
    while (p < e && *p >= '0' && *p <= '9') p++;
  } else {
    return NULL;
  }
  if (p < e && *p == '.') {
    p++;
    if (!(p < e && *p >= '0' && *p <= '9')) return NULL;
    while (p < e && *p >= '0' && *p <= '9') p++;
  }
  if (p < e && (*p == 'e' || *p == 'E')) {
    p++;
    if (p < e && (*p == '+' || *p == '-')) p++;
    if (!(p < e && *p >= '0' && *p <= '9')) return NULL;
    while (p < e && *p >= '0' && *p <= '9') p++;
  }
  /* fast path: at most 19 significant digits in m, |m| < 2^53, and a power of ten within 1e22 */
  uint64_t m = 0;
  int ndig = 0, fracd = 0, ok = 1, seenfrac = 0;
  long expv = 0;
  const unsigned char *q = s + neg;
  for (; q < p; q++) {
    if (*q == '.') {
      seenfrac = 1;
    } else if (*q == 'e' || *q == 'E') {
      q++;
      int eneg = 0;
      if (*q == '+' || *q == '-') eneg = (*q++ == '-');
      while (q < p) {
        if (expv < 100000) expv = expv * 10 + (*q - '0');
        q++;
      }
      if (eneg) expv = -expv;
      break;
    } else {
      if (m == 0 && *q == '0') {
        if (seenfrac) fracd++;
        continue;
      }
      if (++ndig > 19) {
        ok = 0;
        break;
      }
      m = m * 10 + (uint64_t)(*q - '0');
      if (seenfrac) fracd++;
    }
  }
  double v;
  if (ok && m <= (1ull << 53) && expv - fracd >= -22 && expv - fracd <= 22) {
    long E = expv - fracd;
    v = (double)m;
    if (E < 0) v /= P10[-E];
    else if (E > 0) v *= P10[E];
  } else {
    size_t len = (size_t)(p - s);
    char small[64];
    char *tmp = len < sizeof small ? small : malloc(len + 1);
    if (!tmp) die("out of memory");
    memcpy(tmp, s, len);
    tmp[len] = '\0';
    v = strtod(tmp, NULL);
    if (tmp != small) free(tmp);
  }
  JValue *val = jv_node(j->d, J_NUMBER);
  val->u.number = neg ? -v : v;
  j->p = p;
  j->counts->numbers++;
  return val;
}

static JValue *jp_array(JParser *j) {
  JDoc *d = j->d;
  JValue *v = jv_node(d, J_ARRAY);
  j->counts->arrays++;
  j->p++;
  jp_skip(j);
  if (j->p < j->end && *j->p == ']') {
    j->p++;
    return v;
  }
  size_t base = d->isp;
  for (;;) {
    JValue *item = jp_value(j);
    if (!item) return NULL;
    if (d->isp == d->icap) {
      size_t cap = d->icap ? d->icap * 2 : 1024;
      JValue **p = realloc(d->istk, cap * sizeof *p);
      if (!p) die("out of memory");
      d->istk = p;
      d->icap = cap;
    }
    d->istk[d->isp++] = item;
    jp_skip(j);
    if (j->p < j->end && *j->p == ',') {
      j->p++;
      jp_skip(j);
      continue;
    }
    if (j->p < j->end && *j->p == ']') {
      j->p++;
      break;
    }
    return NULL;
  }
  size_t n = d->isp - base;
  v->count = (uint32_t)n;
  v->u.items = arena_alloc(&d->arena, n * sizeof(JValue *));
  memcpy(v->u.items, d->istk + base, n * sizeof(JValue *));
  d->isp = base;
  return v;
}

static JValue *jp_object(JParser *j) {
  JDoc *d = j->d;
  JValue *v = jv_node(d, J_OBJECT);
  j->counts->objects++;
  j->p++;
  jp_skip(j);
  if (j->p < j->end && *j->p == '}') {
    j->p++;
    return v;
  }
  size_t base = d->msp;
  for (;;) {
    jp_skip(j);
    if (!(j->p < j->end && *j->p == '"')) return NULL;
    const char *key;
    size_t klen;
    if (!jp_string(j, &key, &klen)) return NULL;
    jp_skip(j);
    if (!(j->p < j->end && *j->p == ':')) return NULL;
    j->p++;
    JValue *val = jp_value(j);
    if (!val) return NULL;
    if (d->msp == d->mcap) {
      size_t cap = d->mcap ? d->mcap * 2 : 1024;
      JMember *m = realloc(d->mstk, cap * sizeof *m);
      if (!m) die("out of memory");
      d->mstk = m;
      d->mcap = cap;
    }
    JMember *mem = &d->mstk[d->msp++];
    mem->key = key;
    mem->klen = (uint32_t)klen;
    mem->value = val;
    jp_skip(j);
    if (j->p < j->end && *j->p == ',') {
      j->p++;
      continue;
    }
    if (j->p < j->end && *j->p == '}') {
      j->p++;
      break;
    }
    return NULL;
  }
  size_t n = d->msp - base;
  v->count = (uint32_t)n;
  v->u.members = arena_alloc(&d->arena, n * sizeof(JMember));
  memcpy(v->u.members, d->mstk + base, n * sizeof(JMember));
  d->msp = base;
  return v;
}

static JValue *jp_value(JParser *j) {
  jp_skip(j);
  if (j->p >= j->end) return NULL;
  unsigned c = *j->p;
  if (c == '{') return jp_object(j);
  if (c == '[') return jp_array(j);
  if (c == '"') {
    JValue *v = jv_node(j->d, J_STRING);
    if (!jp_string(j, &v->u.str.p, &v->u.str.n)) return NULL;
    j->counts->strings++;
    return v;
  }
  if (c == '-' || (c >= '0' && c <= '9')) return jp_number(j);
  if (j->end - j->p >= 4 && memcmp(j->p, "null", 4) == 0) {
    j->p += 4;
    return jv_node(j->d, J_NULL);
  }
  if (j->end - j->p >= 4 && memcmp(j->p, "true", 4) == 0) {
    j->p += 4;
    return jv_node(j->d, J_TRUE);
  }
  if (j->end - j->p >= 5 && memcmp(j->p, "false", 5) == 0) {
    j->p += 5;
    return jv_node(j->d, J_FALSE);
  }
  return NULL;
}

/* Parses one JSON value from s into d (trailing text after it is ignored). The DOM's strings
 * point into s, so s must outlive d's use. Returns NULL on error. */
static JValue *json_parse(JDoc *d, const char *s, size_t n, JCounts *counts) {
  JParser j;
  j.p = (const unsigned char *)s;
  j.end = j.p + n;
  j.d = d;
  j.counts = counts;
  d->isp = d->msp = 0;
  return jp_value(&j);
}

/* ---------- serializer (JSON.stringify without replacer or indent) ---------- */

static void json_write_string(Buf *b, const char *s, size_t n) {
  buf_reserve(b, n + 2);
  buf_putc(b, '"');
  size_t i = 0, run = 0;
  while (i < n) {
    i = (size_t)(json_scan((const unsigned char *)s + i, (const unsigned char *)s + n, 1) - (const unsigned char *)s);
    if (i >= n) break;
    unsigned char c = (unsigned char)s[i];
    int surrogate = c == 0xED && i + 2 < n && (unsigned char)s[i + 1] >= 0xA0 && (unsigned char)s[i + 1] <= 0xBF;
    if (c == 0xED && !surrogate) {
      i++;
      continue;
    }
    buf_append(b, s + run, i - run);
    if (surrogate) {
      uint32_t cp = ((uint32_t)(c & 0x0F) << 12) | ((uint32_t)((unsigned char)s[i + 1] & 0x3F) << 6) |
                    ((unsigned char)s[i + 2] & 0x3F);
      char e[6];
      static const char hx[] = "0123456789abcdef";
      e[0] = '\\';
      e[1] = 'u';
      e[2] = hx[(cp >> 12) & 15];
      e[3] = hx[(cp >> 8) & 15];
      e[4] = hx[(cp >> 4) & 15];
      e[5] = hx[cp & 15];
      buf_append(b, e, 6);
      i += 3;
    } else {
      switch (c) {
      case '"': buf_append(b, "\\\"", 2); break;
      case '\\': buf_append(b, "\\\\", 2); break;
      case '\b': buf_append(b, "\\b", 2); break;
      case '\f': buf_append(b, "\\f", 2); break;
      case '\n': buf_append(b, "\\n", 2); break;
      case '\r': buf_append(b, "\\r", 2); break;
      case '\t': buf_append(b, "\\t", 2); break;
      default: {
        static const char hx[] = "0123456789abcdef";
        char e[6] = {'\\', 'u', '0', '0', hx[c >> 4], hx[c & 15]};
        buf_append(b, e, 6);
      }
      }
      i++;
    }
    run = i;
  }
  buf_append(b, s + run, n - run);
  buf_putc(b, '"');
}

static void json_write(Buf *b, const JValue *v) {
  switch (v->type) {
  case J_NULL: buf_append(b, "null", 4); break;
  case J_FALSE: buf_append(b, "false", 5); break;
  case J_TRUE: buf_append(b, "true", 4); break;
  case J_NUMBER:
    if (!isfinite(v->u.number)) {
      buf_append(b, "null", 4);
    } else {
      buf_reserve(b, 32);
      b->len += js_number_to_string(v->u.number, b->data + b->len);
    }
    break;
  case J_STRING: json_write_string(b, v->u.str.p, v->u.str.n); break;
  case J_ARRAY:
    buf_putc(b, '[');
    for (uint32_t i = 0; i < v->count; i++) {
      if (i) buf_putc(b, ',');
      json_write(b, v->u.items[i]);
    }
    buf_putc(b, ']');
    break;
  case J_OBJECT:
    buf_putc(b, '{');
    for (uint32_t i = 0; i < v->count; i++) {
      if (i) buf_putc(b, ',');
      json_write_string(b, v->u.members[i].key, v->u.members[i].klen);
      buf_putc(b, ':');
      json_write(b, v->u.members[i].value);
    }
    buf_putc(b, '}');
    break;
  }
}

/* Loads the payload the driver loads: the .z file is inflated and the JSON value is the first
 * '{' or '[' of the decompressed JS source (the file assigns the object to the global obj). The
 * inflated bytes become the document's source, so the DOM's strings stay valid with d. */
static JValue *load_payload(JDoc *d, const char *path, JCounts *counts) {
  size_t zlen;
  unsigned char *z = read_file(path, &zlen);
  unsigned char *raw;
  size_t rawlen;
  if (zlib_inflate(z, zlen, &raw, &rawlen) != 0) die("payload is not a valid zlib stream");
  free(z);
  size_t start = 0;
  while (start < rawlen && raw[start] != '{' && raw[start] != '[') start++;
  d->src = raw;
  JValue *obj = json_parse(d, (const char *)raw + start, rawlen - start, counts);
  if (!obj) die("payload is not JSON");
  return obj;
}

static uint64_t fnv1a(const char *s, size_t n) {
  uint64_t h = 1469598103934665603ull;
  for (size_t i = 0; i < n; i++) {
    h ^= (unsigned char)s[i];
    h *= 1099511628211ull;
  }
  return h;
}

#endif
