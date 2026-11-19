/* Shared pieces for the json-parse-inspector and json-stringify-inspector C analogues of
 * JetStream's SeaMonster rows. Both rows load SeaMonster/inspector-json-payload.js.z, a zlib
 * stream (RFC 1950) around a deflate stream (RFC 1951). This header provides:
 *   - a table-driven inflater, so the C side loads the same compressed bytes the JS driver loads;
 *   - a JSON DOM built in a bump arena with recursive-descent parsing and a serializer. Strings
 *     without escapes point into the source text (as a string view), strings with escapes are
 *     decoded into the arena, and each object/array's children are one contiguous block;
 *   - the ECMAScript Number::toString rule used by JSON.stringify (shortest round-trip digits),
 *     with exact fast paths and a correctly rounded printf/strtod fallback.
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

/* Exact search: the smallest precision whose correctly rounded printf output round-trips. */
static void shortest_printf_loop(double x, int first, char *dig, int *kout, int *nout) {
  char tmp[64];
  for (int prec = first; prec <= 17; prec++) {
    snprintf(tmp, sizeof tmp, "%.*e", prec - 1, x);
    if (prec == 17 || strtod(tmp, NULL) == x) break;
  }
  int k = 0;
  char *e = strchr(tmp, 'e');
  for (char *c = tmp; c < e; c++)
    if (*c >= '0' && *c <= '9') dig[k++] = *c;
  while (k > 1 && dig[k - 1] == '0') k--;
  *kout = k;
  *nout = atoi(e + 1) + 1;
}

/* Finds the shortest digit string that round-trips for finite x > 0, with the JS convention
 * x = digits * 10^(n - k) where digits has k digits and no trailing zero. When several shortest
 * strings exist, the one closest to x is taken. Writes the digits into dig (at least 17 bytes). */
static void shortest_digits(double x, char *dig, int *kout, int *nout) {
  int e2;
  int pow2 = frexp(x, &e2) == 0.5;
  if (!pow2) {
    /* Fast exact path: x rounds to m * 10^-k for a small k with m < 2^50. Such an m is the nearest
     * k-decimal to x (checked with a tolerance; ties fall through), and it round-trips when the
     * double division m / 10^k returns x exactly. The smallest such k gives the fewest digits. */
    for (int k = 0; k <= 22; k++) {
      double y = x * P10[k];
      if (y >= 1125899906842624.0) break; /* 2^50 */
      double fl = floor(y), fr = y - fl;
      if (fabs(fr - 0.5) < 1e-6 + y * 1e-15) break;
      double m = fr < 0.5 ? fl : fl + 1;
      if (m / P10[k] == x) {
        uint64_t mi = (uint64_t)m;
        int kk = k;
        if (mi == 0) break;
        while (mi % 10 == 0) {
          mi /= 10;
          kk--;
        }
        char t[24];
        int L = 0;
        while (mi) {
          t[L++] = (char)('0' + mi % 10);
          mi /= 10;
        }
        for (int i = 0; i < L; i++) dig[i] = t[L - 1 - i];
        *kout = L;
        *nout = L - kk;
        return;
      }
    }
  }
  /* Exact fallback. One printf gives the correctly rounded 17-digit decimal d17 (glibc printf is
   * exact). For a non-power-of-two, the shortest digits are round15(x) if that round-trips, else
   * round16(x) if that round-trips, else d17. The 15 and 16 digit roundings are taken from d17,
   * which equals rounding x itself unless d17's tail is exactly a midpoint; that case, and
   * powers of two (whose lower half-interval is narrower) and subnormals use the exact loop. */
  char tmp[64];
  if (pow2 || x < 2.2250738585072014e-308) { /* subnormals have fewer significant bits */
    shortest_printf_loop(x, 1, dig, kout, nout);
    return;
  }
  snprintf(tmp, sizeof tmp, "%.16e", x);
  char *e = strchr(tmp, 'e');
  char d17[17];
  int m = 0;
  for (char *c = tmp; c < e; c++)
    if (*c >= '0' && *c <= '9') d17[m++] = *c;
  int E = atoi(e + 1);
  for (int p = 15; p <= 16; p++) {
    int tail_zero = 1;
    for (int t = p + 1; t < 17; t++)
      if (d17[t] != '0') tail_zero = 0;
    if (d17[p] == '5' && tail_zero) {
      shortest_printf_loop(x, 1, dig, kout, nout);
      return;
    }
    char c[17];
    memcpy(c, d17, (size_t)p);
    int up = d17[p] >= '5', carry_out = 0;
    if (up) {
      int i = p - 1;
      while (i >= 0 && c[i] == '9') c[i--] = '0';
      if (i >= 0) {
        c[i]++;
      } else {
        carry_out = 1;
        c[0] = '1';
      }
    }
    int ce = carry_out ? E + 1 : E;
    char buf[40];
    snprintf(buf, sizeof buf, "%c.%.*se%d", c[0], p - 1, c + 1, ce);
    if (strtod(buf, NULL) == x) {
      int k2 = p;
      while (k2 > 1 && c[k2 - 1] == '0') k2--;
      memcpy(dig, c, (size_t)k2);
      *kout = k2;
      *nout = ce + 1;
      return;
    }
  }
  int k = 17;
  while (k > 1 && d17[k - 1] == '0') k--;
  memcpy(dig, d17, (size_t)k);
  *kout = k;
  *nout = E + 1;
}

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
  char digits[24];
  int k, n;
  shortest_digits(x, digits, &k, &n);
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
static void doc_reset(JDoc *d) {
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
