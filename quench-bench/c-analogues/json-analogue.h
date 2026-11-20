/* Shared pieces for the json-parse-inspector and json-stringify-inspector C analogues of
 * JetStream's SeaMonster rows. Both rows load SeaMonster/inspector-json-payload.js.z, a zlib
 * stream (RFC 1950) around a deflate stream (RFC 1951). This header provides:
 *   - a small inflater, so the C side loads the same compressed bytes the JS driver loads;
 *   - a JSON DOM with a recursive-descent parser and a serializer, one heap node per JSON
 *     value, as JSON.parse and JSON.stringify allocate one JS object per value;
 *   - the ECMAScript Number::toString rule used by JSON.stringify (shortest round-trip digits).
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
  if (b->len + extra + 1 > b->cap) {
    size_t cap = b->cap ? b->cap : 256;
    while (cap < b->len + extra + 1) cap *= 2;
    char *p = realloc(b->data, cap);
    if (!p) die("out of memory");
    b->data = p;
    b->cap = cap;
  }
}

static void buf_append(Buf *b, const char *s, size_t n) {
  buf_reserve(b, n);
  memcpy(b->data + b->len, s, n);
  b->len += n;
  b->data[b->len] = '\0';
}

static void buf_putc(Buf *b, char c) { buf_append(b, &c, 1); }

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
  size_t cap = 1 << 16, n = 0;
  unsigned char *data = malloc(cap);
  if (!data) die("out of memory");
  for (;;) {
    if (n == cap) {
      cap *= 2;
      unsigned char *p = realloc(data, cap);
      if (!p) die("out of memory");
      data = p;
    }
    size_t got = fread(data + n, 1, cap - n, f);
    if (got == 0) break;
    n += got;
  }
  fclose(f);
  *len = n;
  return data;
}

/* ---------- zlib / DEFLATE inflater (puff-style, portable C) ---------- */

typedef struct {
  const unsigned char *in;
  size_t inlen, pos;
  unsigned bitbuf;
  int bitcnt;
  unsigned char *out;
  size_t outlen, outcap;
  int err;
} Inflate;

typedef struct { short count[16]; short symbol[320]; } Huff;

static const short INF_LBASE[29] = {3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27,
                                    31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258};
static const short INF_LEXT[29] = {0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2,
                                   2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0};
static const short INF_DBASE[30] = {1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129,
                                    193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097,
                                    6145, 8193, 12289, 16385, 24577};
static const short INF_DEXT[30] = {0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6,
                                   6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13};
static const unsigned char INF_ORDER[19] = {16, 17, 18, 0, 8, 7, 9, 6, 10, 5,
                                            11, 4, 12, 3, 13, 2, 14, 1, 15};

static int inf_bit(Inflate *s) {
  if (s->bitcnt == 0) {
    if (s->pos >= s->inlen) {
      s->err = 1;
      return 0;
    }
    s->bitbuf = s->in[s->pos++];
    s->bitcnt = 8;
  }
  int b = (int)(s->bitbuf & 1u);
  s->bitbuf >>= 1;
  s->bitcnt--;
  return b;
}

static unsigned inf_bits(Inflate *s, int n) {
  unsigned v = 0;
  for (int i = 0; i < n; i++) v |= (unsigned)inf_bit(s) << i;
  return v;
}

/* Returns 0 for a complete code, 1 for an incomplete one, -1 for an over-subscribed one. */
static int huff_build(Huff *h, const short *length, int n) {
  int left = 1;
  short offs[16];
  memset(h->count, 0, sizeof h->count);
  for (int sym = 0; sym < n; sym++) h->count[length[sym]]++;
  if (h->count[0] == n) return 0;
  for (int len = 1; len < 16; len++) {
    left <<= 1;
    left -= h->count[len];
    if (left < 0) return -1;
  }
  offs[1] = 0;
  for (int len = 1; len < 15; len++) offs[len + 1] = (short)(offs[len] + h->count[len]);
  for (int sym = 0; sym < n; sym++)
    if (length[sym] != 0) h->symbol[offs[length[sym]]++] = (short)sym;
  return left;
}

static int huff_decode(Inflate *s, const Huff *h) {
  int code = 0, first = 0, index = 0;
  for (int len = 1; len < 16; len++) {
    code |= inf_bit(s);
    int count = h->count[len];
    if (code - count < first) return h->symbol[index + (code - first)];
    index += count;
    first += count;
    first <<= 1;
    code <<= 1;
  }
  s->err = 1;
  return -1;
}

static int inf_put(Inflate *s, unsigned char c) {
  if (s->outlen == s->outcap) {
    size_t cap = s->outcap ? s->outcap * 2 : (1u << 16);
    unsigned char *p = realloc(s->out, cap);
    if (!p) {
      s->err = 1;
      return -1;
    }
    s->out = p;
    s->outcap = cap;
  }
  s->out[s->outlen++] = c;
  return 0;
}

static int inf_codes(Inflate *s, const Huff *lc, const Huff *dc) {
  for (;;) {
    int sym = huff_decode(s, lc);
    if (s->err) return -1;
    if (sym < 256) {
      if (inf_put(s, (unsigned char)sym) < 0) return -1;
    } else if (sym == 256) {
      return 0;
    } else {
      sym -= 257;
      if (sym >= 29) {
        s->err = 1;
        return -1;
      }
      unsigned len = (unsigned)INF_LBASE[sym] + inf_bits(s, INF_LEXT[sym]);
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
      for (unsigned i = 0; i < len; i++)
        if (inf_put(s, s->out[s->outlen - dist]) < 0) return -1;
    }
  }
}

static int inf_fixed(Inflate *s) {
  short lengths[320];
  Huff lc, dc;
  int i = 0;
  for (; i < 144; i++) lengths[i] = 8;
  for (; i < 256; i++) lengths[i] = 9;
  for (; i < 280; i++) lengths[i] = 7;
  for (; i < 288; i++) lengths[i] = 8;
  huff_build(&lc, lengths, 288);
  for (i = 0; i < 30; i++) lengths[i] = 5;
  huff_build(&dc, lengths, 30);
  return inf_codes(s, &lc, &dc);
}

static int inf_dynamic(Inflate *s) {
  short lengths[320];
  Huff lc, dc;
  memset(lengths, 0, sizeof lengths);
  int nlen = (int)inf_bits(s, 5) + 257;
  int ndist = (int)inf_bits(s, 5) + 1;
  int ncode = (int)inf_bits(s, 4) + 4;
  if (nlen > 286 || ndist > 30) {
    s->err = 1;
    return -1;
  }
  for (int i = 0; i < ncode; i++) lengths[INF_ORDER[i]] = (short)inf_bits(s, 3);
  if (huff_build(&lc, lengths, 19) != 0) {
    s->err = 1;
    return -1;
  }
  int index = 0;
  memset(lengths, 0, sizeof lengths);
  while (index < nlen + ndist) {
    int sym = huff_decode(s, &lc);
    if (s->err) return -1;
    if (sym < 16) {
      lengths[index++] = (short)sym;
    } else {
      short len = 0;
      int rep;
      if (sym == 16) {
        if (index == 0) {
          s->err = 1;
          return -1;
        }
        len = lengths[index - 1];
        rep = 3 + (int)inf_bits(s, 2);
      } else if (sym == 17) {
        rep = 3 + (int)inf_bits(s, 3);
      } else {
        rep = 11 + (int)inf_bits(s, 7);
      }
      if (index + rep > nlen + ndist) {
        s->err = 1;
        return -1;
      }
      while (rep--) lengths[index++] = len;
    }
  }
  if (lengths[256] == 0) {
    s->err = 1;
    return -1;
  }
  int err = huff_build(&lc, lengths, nlen);
  if (err < 0 || (err > 0 && nlen - lc.count[0] != 1)) {
    s->err = 1;
    return -1;
  }
  err = huff_build(&dc, lengths + nlen, ndist);
  if (err < 0 || (err > 0 && ndist - dc.count[0] != 1)) {
    s->err = 1;
    return -1;
  }
  return inf_codes(s, &lc, &dc);
}

static int inf_stored(Inflate *s) {
  s->bitcnt = 0;
  if (s->pos + 4 > s->inlen) {
    s->err = 1;
    return -1;
  }
  size_t len = s->in[s->pos] | ((size_t)s->in[s->pos + 1] << 8);
  s->pos += 4;
  if (s->pos + len > s->inlen) {
    s->err = 1;
    return -1;
  }
  for (size_t i = 0; i < len; i++)
    if (inf_put(s, s->in[s->pos + i]) < 0) return -1;
  s->pos += len;
  return 0;
}

/* Inflates a zlib stream. On success *out is malloc'd and must be freed by the caller. */
static int zlib_inflate(const unsigned char *in, size_t inlen, unsigned char **out, size_t *outlen) {
  if (inlen < 6 || (in[0] & 0x0F) != 8 || (((unsigned)in[0] << 8) | in[1]) % 31 != 0) return -1;
  Inflate s;
  memset(&s, 0, sizeof s);
  s.in = in + 2;
  s.inlen = inlen - 2;
  int last;
  do {
    last = inf_bit(&s);
    unsigned type = inf_bits(&s, 2);
    if (s.err) break;
    if (type == 0) inf_stored(&s);
    else if (type == 1) inf_fixed(&s);
    else if (type == 2) inf_dynamic(&s);
    else s.err = 1;
  } while (!s.err && !last);
  if (s.err) {
    free(s.out);
    return -1;
  }
  if (!s.out) s.out = malloc(1);
  *out = s.out;
  *outlen = s.outlen;
  return 0;
}

/* ---------- ECMAScript Number::toString ---------- */

/* Writes the JS string form of x (at least 32 bytes in out) and returns its length. The digits
 * are the shortest that round-trip, as in ECMA-262 Number::toString. */
static size_t js_number_to_string(double x, char *out) {
  if (x != x) {
    strcpy(out, "NaN");
    return 3;
  }
  if (isinf(x)) {
    if (x < 0) {
      strcpy(out, "-Infinity");
      return 9;
    }
    strcpy(out, "Infinity");
    return 8;
  }
  if (x == 0) {
    strcpy(out, "0");
    return 1;
  }
  char *p = out;
  if (x < 0) {
    *p++ = '-';
    x = -x;
  }
  char tmp[64];
  int prec;
  for (prec = 1; prec <= 17; prec++) {
    snprintf(tmp, sizeof tmp, "%.*e", prec - 1, x);
    if (strtod(tmp, NULL) == x) break;
  }
  char digits[24];
  int k = 0;
  char *e = strchr(tmp, 'e');
  for (char *c = tmp; c < e; c++)
    if (*c >= '0' && *c <= '9') digits[k++] = *c;
  while (k > 1 && digits[k - 1] == '0') k--;
  int n = atoi(e + 1) + 1;
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
    p += sprintf(p, "%d", ee < 0 ? -ee : ee);
  }
  *p = '\0';
  return (size_t)(p - out);
}

/* ---------- JSON DOM ---------- */

typedef enum { J_NULL, J_FALSE, J_TRUE, J_NUMBER, J_STRING, J_ARRAY, J_OBJECT } JType;
typedef struct JValue JValue;
typedef struct { char *key; size_t klen; JValue *value; } JMember;
struct JValue {
  JType type;
  double number;
  char *str;
  size_t slen;
  JValue **items;   /* J_ARRAY elements */
  JMember *members; /* J_OBJECT members, insertion order */
  size_t count, cap;
};

typedef struct { size_t objects, arrays, strings, numbers; } JCounts;

static JValue *jv_new(JType t) {
  JValue *v = calloc(1, sizeof *v);
  if (!v) die("out of memory");
  v->type = t;
  return v;
}

static void jv_free(JValue *v) {
  if (!v) return;
  if (v->type == J_STRING) {
    free(v->str);
  } else if (v->type == J_ARRAY) {
    for (size_t i = 0; i < v->count; i++) jv_free(v->items[i]);
    free(v->items);
  } else if (v->type == J_OBJECT) {
    for (size_t i = 0; i < v->count; i++) {
      free(v->members[i].key);
      jv_free(v->members[i].value);
    }
    free(v->members);
  }
  free(v);
}

/* ---------- parser ---------- */

typedef struct {
  const unsigned char *p, *end;
  JCounts *counts;
} JParser;

static JValue *jp_value(JParser *j);

static void jp_skip(JParser *j) {
  while (j->p < j->end && (*j->p == ' ' || *j->p == '\t' || *j->p == '\n' || *j->p == '\r')) j->p++;
}

static int jp_hex4(JParser *j, uint32_t *cp) {
  if (j->end - j->p < 4) return 0;
  uint32_t v = 0;
  for (int i = 0; i < 4; i++) {
    unsigned c = j->p[i], d;
    if (c >= '0' && c <= '9') d = c - '0';
    else if (c >= 'a' && c <= 'f') d = c - 'a' + 10;
    else if (c >= 'A' && c <= 'F') d = c - 'A' + 10;
    else return 0;
    v = v * 16 + d;
  }
  j->p += 4;
  *cp = v;
  return 1;
}

/* Appends code point cp as UTF-8; lone surrogates are kept as 3-byte sequences (WTF-8). */
static void utf8_put(Buf *b, uint32_t cp) {
  if (cp < 0x80) {
    buf_putc(b, (char)cp);
  } else if (cp < 0x800) {
    buf_putc(b, (char)(0xC0 | (cp >> 6)));
    buf_putc(b, (char)(0x80 | (cp & 0x3F)));
  } else if (cp < 0x10000) {
    buf_putc(b, (char)(0xE0 | (cp >> 12)));
    buf_putc(b, (char)(0x80 | ((cp >> 6) & 0x3F)));
    buf_putc(b, (char)(0x80 | (cp & 0x3F)));
  } else {
    buf_putc(b, (char)(0xF0 | (cp >> 18)));
    buf_putc(b, (char)(0x80 | ((cp >> 12) & 0x3F)));
    buf_putc(b, (char)(0x80 | ((cp >> 6) & 0x3F)));
    buf_putc(b, (char)(0x80 | (cp & 0x3F)));
  }
}

/* Parses a string literal at j->p (which must be '"'). Returns a malloc'd NUL-terminated copy. */
static char *jp_string(JParser *j, size_t *outlen) {
  Buf b = {NULL, 0, 0};
  j->p++;
  for (;;) {
    if (j->p >= j->end) goto fail;
    unsigned c = *j->p++;
    if (c == '"') break;
    if (c < 0x20) goto fail;
    if (c != '\\') {
      buf_putc(&b, (char)c);
      continue;
    }
    if (j->p >= j->end) goto fail;
    c = *j->p++;
    switch (c) {
    case '"': case '\\': case '/': buf_putc(&b, (char)c); break;
    case 'b': buf_putc(&b, '\b'); break;
    case 'f': buf_putc(&b, '\f'); break;
    case 'n': buf_putc(&b, '\n'); break;
    case 'r': buf_putc(&b, '\r'); break;
    case 't': buf_putc(&b, '\t'); break;
    case 'u': {
      uint32_t cp;
      if (!jp_hex4(j, &cp)) goto fail;
      if (cp >= 0xD800 && cp <= 0xDBFF && j->end - j->p >= 6 && j->p[0] == '\\' && j->p[1] == 'u') {
        const unsigned char *save = j->p;
        j->p += 2;
        uint32_t lo;
        if (jp_hex4(j, &lo) && lo >= 0xDC00 && lo <= 0xDFFF)
          cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
        else
          j->p = save;
      }
      utf8_put(&b, cp);
      break;
    }
    default: goto fail;
    }
  }
  buf_reserve(&b, 0);
  b.data[b.len] = '\0';
  *outlen = b.len;
  return b.data;
fail:
  buf_free(&b);
  return NULL;
}

static JValue *jp_number(JParser *j) {
  const unsigned char *s = j->p;
  if (j->p < j->end && *j->p == '-') j->p++;
  if (j->p < j->end && *j->p == '0') {
    j->p++;
  } else if (j->p < j->end && *j->p >= '1' && *j->p <= '9') {
    while (j->p < j->end && *j->p >= '0' && *j->p <= '9') j->p++;
  } else {
    return NULL;
  }
  if (j->p < j->end && *j->p == '.') {
    j->p++;
    if (!(j->p < j->end && *j->p >= '0' && *j->p <= '9')) return NULL;
    while (j->p < j->end && *j->p >= '0' && *j->p <= '9') j->p++;
  }
  if (j->p < j->end && (*j->p == 'e' || *j->p == 'E')) {
    j->p++;
    if (j->p < j->end && (*j->p == '+' || *j->p == '-')) j->p++;
    if (!(j->p < j->end && *j->p >= '0' && *j->p <= '9')) return NULL;
    while (j->p < j->end && *j->p >= '0' && *j->p <= '9') j->p++;
  }
  size_t n = (size_t)(j->p - s);
  char small[64];
  char *tmp = n < sizeof small ? small : malloc(n + 1);
  if (!tmp) die("out of memory");
  memcpy(tmp, s, n);
  tmp[n] = '\0';
  JValue *v = jv_new(J_NUMBER);
  v->number = strtod(tmp, NULL);
  if (tmp != small) free(tmp);
  j->counts->numbers++;
  return v;
}

static JValue *jp_array(JParser *j) {
  JValue *v = jv_new(J_ARRAY);
  j->counts->arrays++;
  j->p++;
  jp_skip(j);
  if (j->p < j->end && *j->p == ']') {
    j->p++;
    return v;
  }
  for (;;) {
    JValue *item = jp_value(j);
    if (!item) goto fail;
    if (v->count == v->cap) {
      size_t cap = v->cap ? v->cap * 2 : 4;
      JValue **p = realloc(v->items, cap * sizeof *p);
      if (!p) die("out of memory");
      v->items = p;
      v->cap = cap;
    }
    v->items[v->count++] = item;
    jp_skip(j);
    if (j->p < j->end && *j->p == ',') {
      j->p++;
      jp_skip(j);
      continue;
    }
    if (j->p < j->end && *j->p == ']') {
      j->p++;
      return v;
    }
    goto fail;
  }
fail:
  jv_free(v);
  return NULL;
}

static JValue *jp_object(JParser *j) {
  JValue *v = jv_new(J_OBJECT);
  j->counts->objects++;
  j->p++;
  jp_skip(j);
  if (j->p < j->end && *j->p == '}') {
    j->p++;
    return v;
  }
  for (;;) {
    jp_skip(j);
    if (!(j->p < j->end && *j->p == '"')) goto fail;
    size_t klen;
    char *key = jp_string(j, &klen);
    if (!key) goto fail;
    jp_skip(j);
    if (!(j->p < j->end && *j->p == ':')) {
      free(key);
      goto fail;
    }
    j->p++;
    JValue *val = jp_value(j);
    if (!val) {
      free(key);
      goto fail;
    }
    if (v->count == v->cap) {
      size_t cap = v->cap ? v->cap * 2 : 4;
      JMember *m = realloc(v->members, cap * sizeof *m);
      if (!m) die("out of memory");
      v->members = m;
      v->cap = cap;
    }
    v->members[v->count].key = key;
    v->members[v->count].klen = klen;
    v->members[v->count].value = val;
    v->count++;
    jp_skip(j);
    if (j->p < j->end && *j->p == ',') {
      j->p++;
      continue;
    }
    if (j->p < j->end && *j->p == '}') {
      j->p++;
      return v;
    }
    goto fail;
  }
fail:
  jv_free(v);
  return NULL;
}

static JValue *jp_value(JParser *j) {
  jp_skip(j);
  if (j->p >= j->end) return NULL;
  unsigned c = *j->p;
  if (c == '{') return jp_object(j);
  if (c == '[') return jp_array(j);
  if (c == '"') {
    JValue *v = jv_new(J_STRING);
    v->str = jp_string(j, &v->slen);
    if (!v->str) {
      free(v);
      return NULL;
    }
    j->counts->strings++;
    return v;
  }
  if (c == '-' || (c >= '0' && c <= '9')) return jp_number(j);
  if (j->end - j->p >= 4 && memcmp(j->p, "null", 4) == 0) {
    j->p += 4;
    return jv_new(J_NULL);
  }
  if (j->end - j->p >= 4 && memcmp(j->p, "true", 4) == 0) {
    j->p += 4;
    return jv_new(J_TRUE);
  }
  if (j->end - j->p >= 5 && memcmp(j->p, "false", 5) == 0) {
    j->p += 5;
    return jv_new(J_FALSE);
  }
  return NULL;
}

/* Parses one JSON value from s (trailing text after it is ignored). Returns NULL on error. */
static JValue *json_parse(const char *s, size_t n, JCounts *counts) {
  JParser j;
  j.p = (const unsigned char *)s;
  j.end = j.p + n;
  j.counts = counts;
  return jp_value(&j);
}

/* ---------- serializer (JSON.stringify without replacer or indent) ---------- */

static void json_write_string(Buf *b, const char *s, size_t n) {
  buf_putc(b, '"');
  for (size_t i = 0; i < n; i++) {
    unsigned char c = (unsigned char)s[i];
    if (c == '"') {
      buf_append(b, "\\\"", 2);
    } else if (c == '\\') {
      buf_append(b, "\\\\", 2);
    } else if (c == '\b') {
      buf_append(b, "\\b", 2);
    } else if (c == '\f') {
      buf_append(b, "\\f", 2);
    } else if (c == '\n') {
      buf_append(b, "\\n", 2);
    } else if (c == '\r') {
      buf_append(b, "\\r", 2);
    } else if (c == '\t') {
      buf_append(b, "\\t", 2);
    } else if (c < 0x20) {
      char e[8];
      snprintf(e, sizeof e, "\\u%04x", c);
      buf_append(b, e, 6);
    } else if (c == 0xED && i + 2 < n && (unsigned char)s[i + 1] >= 0xA0 && (unsigned char)s[i + 1] <= 0xBF) {
      uint32_t cp = ((uint32_t)(c & 0x0F) << 12) | ((uint32_t)((unsigned char)s[i + 1] & 0x3F) << 6) |
                    ((unsigned char)s[i + 2] & 0x3F);
      char e[8];
      snprintf(e, sizeof e, "\\u%04x", (unsigned)cp);
      buf_append(b, e, 6);
      i += 2;
    } else {
      buf_putc(b, (char)c);
    }
  }
  buf_putc(b, '"');
}

static void json_write(Buf *b, const JValue *v) {
  switch (v->type) {
  case J_NULL: buf_append(b, "null", 4); break;
  case J_FALSE: buf_append(b, "false", 5); break;
  case J_TRUE: buf_append(b, "true", 4); break;
  case J_NUMBER: {
    if (!isfinite(v->number)) {
      buf_append(b, "null", 4);
    } else {
      char t[40];
      size_t n = js_number_to_string(v->number, t);
      buf_append(b, t, n);
    }
    break;
  }
  case J_STRING: json_write_string(b, v->str, v->slen); break;
  case J_ARRAY:
    buf_putc(b, '[');
    for (size_t i = 0; i < v->count; i++) {
      if (i) buf_putc(b, ',');
      json_write(b, v->items[i]);
    }
    buf_putc(b, ']');
    break;
  case J_OBJECT:
    buf_putc(b, '{');
    for (size_t i = 0; i < v->count; i++) {
      if (i) buf_putc(b, ',');
      json_write_string(b, v->members[i].key, v->members[i].klen);
      buf_putc(b, ':');
      json_write(b, v->members[i].value);
    }
    buf_putc(b, '}');
    break;
  }
}

/* Loads the payload the driver loads: the .z file is inflated and the JSON value is the first
 * '{' or '[' of the decompressed JS source (the file assigns the object to the global obj). */
static JValue *load_payload(const char *path, JCounts *counts) {
  size_t zlen;
  unsigned char *z = read_file(path, &zlen);
  unsigned char *raw;
  size_t rawlen;
  if (zlib_inflate(z, zlen, &raw, &rawlen) != 0) die("payload is not a valid zlib stream");
  free(z);
  size_t start = 0;
  while (start < rawlen && raw[start] != '{' && raw[start] != '[') start++;
  JValue *obj = json_parse((const char *)raw + start, rawlen - start, counts);
  free(raw);
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
