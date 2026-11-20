// jetstream-row: regexp-octane
// jetstream-files: Octane/regexp.js
// iterations: 120
// jetstream-driver: deterministic=true
// c-sources: -lpcre2-16
/* C analogue of JetStream's Octane/regexp.js: the same 237 regular expressions,
 * the same input strings (the setup statements are translated in source order,
 * with the computeInputVariants() arrays built by replaying JetStream's
 * deterministic Math.random), and the same twelve run blocks. Each RegExp is
 * translated to PCRE2 syntax and run by PCRE2-JIT (16-bit library, non-UTF mode:
 * JS matches UTF-16 code units and these patterns have no u flag). Each exec()
 * becomes a call that returns the sum of the lengths of the captures, as Exec()
 * does in JS; replace/split/match follow the JS global-iteration rules.
 * The checksum 1666156 must match on every inner run or the program exits 1.
 * main() does argv[1] (default ITERATIONS) timed runs of the five-run
 * benchmark body, and prints the elapsed milliseconds and the checksum. */
#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>


#define ITERATIONS 120
#define EXPECTED_SUM 1666156

struct WideStr;
typedef struct {
  const char *p;
  size_t n;
  struct WideStr *w; /* the string's UTF-16 form, once made (see wide_lookup) */
} Str;

static int failures;

/* JetStream's deterministic Math.random: Robert Jenkins' 32-bit hash. */
static uint32_t js_seed = 49734321u;

static double js_random(void) {
  uint32_t s = js_seed;
  s = (s + 0x7ed55d16u) + (s << 12);
  s = (s ^ 0xc761c23cu) ^ (s >> 19);
  s = (s + 0x165667b1u) + (s << 5);
  s = (s + 0xd3a2646cu) ^ (s << 9);
  s = (s + 0xfd7046c5u) + (s << 3);
  s = (s ^ 0xb55a4f09u) ^ (s >> 16);
  js_seed = s;
  return (double)s / 4294967296.0;
}

/* computeInputVariants(str, n): variant 0 is str, each later variant replaces
 * one character of its predecessor's source string at a random position. */
static Str *compute_variants(Str str, int n) {
  Str *v = calloc((size_t)n, sizeof(Str));
  if (!v) {
    fprintf(stderr, "regexp-octane: out of memory\n");
    exit(1);
  }
  v[0] = str;
  for (int i = 1; i < n; i++) {
    size_t pos = (size_t)(js_random() * (double)str.n);
    unsigned code = (unsigned char)str.p[pos];
    unsigned add = (unsigned)(js_random() * 128.0);
    char *buf = malloc(str.n);
    if (!buf) {
      fprintf(stderr, "regexp-octane: out of memory\n");
      exit(1);
    }
    memcpy(buf, str.p, pos);
    buf[pos] = (char)((code + add) % 128);
    memcpy(buf + pos + 1, str.p + pos + 1, str.n - pos - 1);
    v[i].p = buf;
    v[i].n = str.n;
  }
  return v;
}

static double now_ms(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (double)ts.tv_sec * 1e3 + (double)ts.tv_nsec / 1e6;
}

/* ---- Regular expressions: JavaScript RegExp sources translated to PCRE2-JIT ---- */
/* A JavaScript RegExp matches UTF-16 code units (no u flag here), so the matcher is
 * PCRE2's 16-bit library in its non-UTF mode, which matches code units exactly as JS
 * does. The input strings are UTF-8 literals; each subject is converted to UTF-16 once
 * per exec/replace/split/match call (the global loops reuse that conversion), and all
 * lengths and offsets are in code units, as in JS. Each JS pattern is translated once,
 * in RegExpSetup, to PCRE2 syntax with the JS meaning: JS \s and '.' sets, $ at the end
 * of input only, ASCII \w \d \b (no UCP), backreferences to groups that did not
 * participate match the empty string (PCRE2_MATCH_UNSET_BACKREF). The pattern is compiled
 * with pcre2_compile and pcre2_jit_compile once, outside the timed loop. */

#define PCRE2_CODE_UNIT_WIDTH 16
#include <ctype.h>
#include <pcre2.h>

/* JS \s: WhiteSpace and LineTerminator code units. */
#define JS_SPACE "\\t\\n\\x{0b}\\f\\r \\x{a0}\\x{1680}\\x{2000}-\\x{200a}\\x{2028}\\x{2029}\\x{202f}\\x{205f}\\x{3000}\\x{feff}"
/* JS '.': any code unit except LineTerminator (\n \r    ). */
#define JS_DOT "[^\\n\\r\\x{2028}\\x{2029}]"

typedef struct {
  char *s;
  size_t n, cap;
} Buf;

typedef struct {
  pcre2_code *code;
  pcre2_match_data *md;
  int ngroups;
  int global;
} Rx;

static pcre2_match_context *rx_mctx;

/* The subject of the current exec/replace/split/match call, as UTF-16 code units. */
static PCRE2_UCHAR *subj;
static size_t subj_len;

static void buf_put(Buf *b, const char *s, size_t n) {
  if (b->n + n + 1 > b->cap) {
    b->cap = (b->n + n + 1) * 2;
    b->s = realloc(b->s, b->cap);
    if (!b->s) {
      fprintf(stderr, "regexp-octane: out of memory\n");
      exit(1);
    }
  }
  memcpy(b->s + b->n, s, n);
  b->n += n;
  b->s[b->n] = '\0';
}

static void buf_str(Buf *b, const char *s) { buf_put(b, s, strlen(s)); }

static void rx_die(const char *what) {
  fprintf(stderr, "regexp-octane: %s\n", what);
  exit(1);
}

static int hex_value(int c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  return -1;
}

/* Reads exactly `digits` hex digits at src[i]; returns the value, advances i. */
static unsigned hex_run(const char *src, size_t n, size_t *i, int digits) {
  unsigned v = 0;
  for (int k = 0; k < digits; k++) {
    int h = *i < n ? hex_value((unsigned char)src[*i]) : -1;
    if (h < 0) rx_die("bad hex escape in regexp source");
    v = v * 16 + (unsigned)h;
    (*i)++;
  }
  return v;
}

/* Translates a JavaScript pattern (no u or v flag) to PCRE2 syntax for 16-bit non-UTF mode. */
static void js_to_pcre2(const char *src, size_t n, Buf *out) {
  size_t i = 0;
  int in_class = 0;
  char tmp[32];
  while (i < n) {
    unsigned char c = (unsigned char)src[i];
    if (c == '\\') {
      unsigned char e;
      if (i + 1 >= n) rx_die("trailing backslash in regexp source");
      e = (unsigned char)src[i + 1];
      i += 2;
      switch (e) {
        case 's':
          buf_str(out, in_class ? JS_SPACE : "[" JS_SPACE "]");
          break;
        case 'S':
          if (in_class) rx_die("\\S inside a class is not translated");
          buf_str(out, "[^" JS_SPACE "]");
          break;
        case 'b':
          buf_str(out, in_class ? "\\x{08}" : "\\b");
          break;
        case 'B':
          buf_str(out, "\\B");
          break;
        case 'd': case 'D': case 'w': case 'W':
        case 'n': case 'r': case 't': case 'f':
          buf_put(out, "\\", 1);
          buf_put(out, (const char *)&e, 1);
          break;
        case 'v':
          buf_str(out, "\\x{0b}");
          break;
        case '0':
          buf_str(out, "\\x{0}");
          break;
        case 'x':
          snprintf(tmp, sizeof tmp, "\\x{%x}", hex_run(src, n, &i, 2));
          buf_str(out, tmp);
          break;
        case 'u':
          snprintf(tmp, sizeof tmp, "\\x{%x}", hex_run(src, n, &i, 4));
          buf_str(out, tmp);
          break;
        case 'c': {
          unsigned letter;
          if (i >= n || !isalpha((unsigned char)src[i])) rx_die("bad \\c escape");
          letter = (unsigned char)src[i++];
          snprintf(tmp, sizeof tmp, "\\x{%x}", letter % 32u);
          buf_str(out, tmp);
          break;
        }
        default:
          if (e >= '1' && e <= '9') {
            if (in_class) rx_die("backreference inside a class");
            buf_put(out, "\\", 1);
            buf_put(out, (const char *)&e, 1);
          } else if (isalpha(e)) {
            /* JS identity escape of a letter: the letter itself */
            buf_put(out, (const char *)&e, 1);
          } else {
            /* JS identity escape of a punctuation character: literal in PCRE2 too */
            buf_put(out, "\\", 1);
            buf_put(out, (const char *)&e, 1);
          }
          break;
      }
      continue;
    }
    if (in_class) {
      if (c == ']') {
        in_class = 0;
        buf_put(out, "]", 1);
      } else if (c == '[') {
        buf_str(out, "\\[");
      } else {
        buf_put(out, (const char *)&c, 1);
      }
      i++;
      continue;
    }
    switch (c) {
      case '[':
        in_class = 1;
        buf_put(out, "[", 1);
        i++;
        if (i < n && src[i] == '^') {
          buf_put(out, "^", 1);
          i++;
        }
        break;
      case '.':
        buf_str(out, JS_DOT);
        i++;
        break;
      case '$':
        buf_str(out, "\\z"); /* no m flag: $ is the end of input only */
        i++;
        break;
      default:
        buf_put(out, (const char *)&c, 1);
        i++;
        break;
    }
  }
}

/* Compiles one JS RegExp (source bytes and flags g, i) for PCRE2-JIT. */
static Rx *rx_compile(const char *src, size_t n, const char *flags) {
  static pcre2_jit_stack *jit_stack;
  Buf pat = {0};
  PCRE2_UCHAR *wide;
  uint32_t opts = PCRE2_MATCH_UNSET_BACKREF;
  uint32_t ng = 0;
  int err = 0;
  PCRE2_SIZE eoff = 0;
  Rx *rx = calloc(1, sizeof *rx);
  if (!rx) rx_die("out of memory");
  for (const char *f = flags; *f; f++) {
    if (*f == 'i') opts |= PCRE2_CASELESS;
    else if (*f == 'g') rx->global = 1;
    else rx_die("regexp flag not used by this benchmark");
  }
  if (!rx_mctx) {
    jit_stack = pcre2_jit_stack_create(32 * 1024, 4 * 1024 * 1024, NULL);
    rx_mctx = pcre2_match_context_create(NULL);
    if (!jit_stack || !rx_mctx) rx_die("cannot create PCRE2 JIT stack");
    pcre2_jit_stack_assign(rx_mctx, NULL, jit_stack);
    pcre2_set_match_limit(rx_mctx, 0xffffffffu); /* JS has no match limit */
  }
  js_to_pcre2(src, n, &pat);
  /* the translated pattern is ASCII: widen it to code units for the 16-bit library */
  wide = malloc((pat.n + 1) * sizeof *wide);
  if (!wide) rx_die("out of memory");
  for (size_t k = 0; k < pat.n; k++) wide[k] = (unsigned char)pat.s[k];
  rx->code = pcre2_compile(wide, pat.n, opts, &err, &eoff, NULL);
  if (!rx->code) {
    PCRE2_UCHAR msg[256];
    pcre2_get_error_message(err, msg, sizeof msg);
    fprintf(stderr, "regexp-octane: %s at %zu in /%s/\n", (const char *)msg, (size_t)eoff, pat.s);
    exit(1);
  }
  if (pcre2_jit_compile(rx->code, PCRE2_JIT_COMPLETE) < 0) rx_die("pcre2_jit_compile failed");
  rx->md = pcre2_match_data_create_from_pattern(rx->code, NULL);
  if (!rx->md) rx_die("out of memory");
  pcre2_pattern_info(rx->code, PCRE2_INFO_CAPTURECOUNT, &ng);
  rx->ngroups = (int)ng;
  free(wide);
  free(pat.s);
  return rx;
}

/* Length of a UTF-8 literal in JS string units (UTF-16 code units). */
static size_t js_length_utf8(Str *s) {
  size_t units = 0;
  for (size_t i = 0; i < s->n; i++) {
    unsigned char c = (unsigned char)s->p[i];
    if ((c & 0xC0) != 0x80) units += (c >= 0xF0) ? 2 : 1;
  }
  return units;
}

/* A JS engine holds each string in its in-memory form (Latin-1 or UTF-16) from the moment it is
 * created, so matching never re-encodes the subject. The C sources keep the strings as UTF-8
 * literals; each distinct string (keyed by its stable address and length: literals and the
 * setup-time variants never move) is converted to UTF-16 once, on first use, and kept. */
typedef struct WideStr { const char *p; size_t n; PCRE2_UCHAR *w; size_t wn; } WideStr;
static WideStr **wide_tab; /* open addressing; entries are separately allocated, so Str.w stays valid */
static size_t wide_cap, wide_count;

static size_t wide_hash(const char *p, size_t n) {
  return (size_t)((((uintptr_t)p >> 3) * 0x9E3779B97F4A7C15ull) ^ n);
}

static void wide_grow(void) {
  size_t old_cap = wide_cap;
  WideStr **old = wide_tab;
  wide_cap = old_cap ? old_cap * 2 : 8192;
  wide_tab = calloc(wide_cap, sizeof *wide_tab);
  if (!wide_tab) rx_die("out of memory");
  for (size_t i = 0; i < old_cap; i++)
    if (old[i]) {
      size_t h = wide_hash(old[i]->p, old[i]->n) & (wide_cap - 1);
      while (wide_tab[h]) h = (h + 1) & (wide_cap - 1);
      wide_tab[h] = old[i];
    }
  free(old);
}

static WideStr *wide_lookup(Str *s) {
  if (s->w) return s->w;
  if ((wide_count + 1) * 2 >= wide_cap) wide_grow();
  size_t h = wide_hash(s->p, s->n) & (wide_cap - 1);
  for (WideStr *e; (e = wide_tab[h]); h = (h + 1) & (wide_cap - 1))
    if (e->p == s->p && e->n == s->n) return s->w = e;
  WideStr *e = malloc(sizeof *e);
  size_t need = js_length_utf8(s), k = 0;
  if (!e || !(e->w = malloc((need + 1) * sizeof *e->w))) rx_die("out of memory");
  for (size_t i = 0; i < s->n;) {
    unsigned char c = (unsigned char)s->p[i];
    unsigned cp;
    int extra;
    if (c < 0x80) { cp = c; extra = 0; }
    else if (c < 0xE0) { cp = c & 0x1Fu; extra = 1; }
    else if (c < 0xF0) { cp = c & 0x0Fu; extra = 2; }
    else { cp = c & 0x07u; extra = 3; }
    for (int j = 1; j <= extra; j++) cp = (cp << 6) | ((unsigned char)s->p[i + (size_t)j] & 0x3Fu);
    i += (size_t)extra + 1;
    if (cp >= 0x10000u) {
      cp -= 0x10000u;
      e->w[k++] = (PCRE2_UCHAR)(0xD800u + (cp >> 10));
      e->w[k++] = (PCRE2_UCHAR)(0xDC00u + (cp & 0x3FFu));
    } else {
      e->w[k++] = (PCRE2_UCHAR)cp;
    }
  }
  e->p = s->p;
  e->n = s->n;
  e->wn = k;
  wide_tab[h] = e;
  wide_count++;
  return s->w = e;
}

/* Length of the string in JS units. */
static size_t js_length(Str *s) { return wide_lookup(s)->wn; }

/* Selects the subject of the current call (from == 0) as its UTF-16 form. */
static void subject_convert(Str *s) {
  WideStr *e = wide_lookup(s);
  subj = e->w;
  subj_len = e->wn;
}

/* Finds the first match starting at or after from (anchored: only at from), in the
 * subject selected by the call's first lookup (from == 0 selects). */
static int rx_find(Rx *rx, Str *s, size_t from, int anchored) {
  int rc;
  PCRE2_SIZE *ov;
  if (from == 0) subject_convert(s);
  if (from > subj_len) return 0;
  rc = pcre2_jit_match(rx->code, subj, (PCRE2_SIZE)subj_len, (PCRE2_SIZE)from,
                   anchored ? PCRE2_ANCHORED : 0, rx->md, rx_mctx);
  if (rc == PCRE2_ERROR_NOMATCH) return 0;
  if (rc < 0) rx_die("pcre2_match failed");
  ov = pcre2_get_ovector_pointer(rx->md);
  for (int g = rc; g <= rx->ngroups; g++) ov[2 * g] = ov[2 * g + 1] = PCRE2_UNSET;
  return 1;
}

/* exec(): sum of the lengths of the match and its captured strings (undefined ones add 0). */
static size_t rx_exec_len(Rx *rx, Str *s) {
  PCRE2_SIZE *ov;
  size_t sum = 0;
  if (!rx_find(rx, s, 0, 0)) return 0;
  ov = pcre2_get_ovector_pointer(rx->md);
  for (int g = 0; g <= rx->ngroups; g++)
    if (ov[2 * g] != PCRE2_UNSET) sum += (size_t)(ov[2 * g + 1] - ov[2 * g]);
  return sum;
}

/* replace(re, ''): builds the result (the kept segments, as the JS string the engine makes) and
 * returns its length in JS units. */
static PCRE2_UCHAR *rep_buf;
static size_t rep_cap;
static size_t rx_replace_len(Rx *rx, Str *s) {
  size_t total = js_length(s), pos = 0, kept = 0, out = 0;
  const PCRE2_UCHAR *src;
  if (total + 1 > rep_cap) {
    rep_cap = (total + 1) * 2;
    rep_buf = realloc(rep_buf, rep_cap * sizeof *rep_buf);
    if (!rep_buf) rx_die("out of memory");
  }
  if (!rx->global) {
    if (rx_find(rx, s, 0, 0)) {
      PCRE2_SIZE *ov = pcre2_get_ovector_pointer(rx->md);
      src = subj;
      memcpy(rep_buf, src, ov[0] * sizeof *src);
      memcpy(rep_buf + ov[0], src + ov[1], (total - ov[1]) * sizeof *src);
      return total - (size_t)(ov[1] - ov[0]);
    }
    return total;
  }
  while (pos <= total && rx_find(rx, s, pos, 0)) {
    PCRE2_SIZE *ov = pcre2_get_ovector_pointer(rx->md);
    size_t st = ov[0], en = ov[1];
    memcpy(rep_buf + out, subj + kept, (st - kept) * sizeof *subj);
    out += st - kept;
    kept = en;
    pos = (en == st) ? en + 1 : en; /* AdvanceStringIndex, one code unit */
  }
  memcpy(rep_buf + out, subj + kept, (total - kept) * sizeof *subj);
  out += total - kept;
  return out;
}

/* split(re): number of elements, captures included (undefined ones count). */
static size_t rx_split_len(Rx *rx, Str *s) {
  size_t size = js_length(s), p = 0, q = 0, count = 0;
  if (size == 0) return rx_find(rx, s, 0, 1) ? 0 : 1;
  while (q < size) {
    PCRE2_SIZE *ov;
    size_t st, e;
    if (!rx_find(rx, s, q, 0)) break;
    ov = pcre2_get_ovector_pointer(rx->md);
    st = ov[0];
    if (st >= size) break;
    e = ov[1];
    if (e == p) {
      q = st + 1;
      continue;
    }
    count += 1 + (size_t)rx->ngroups;
    p = e;
    q = p;
  }
  return count + 1;
}

/* match(re) with the g flag: number of matches (the array is not used). */
static size_t rx_match_all(Rx *rx, Str *s) {
  size_t count = 0, pos = 0;
  if (!rx->global) return rx_find(rx, s, 0, 0) ? 1 : 0;
  while (pos <= js_length(s) && rx_find(rx, s, pos, 0)) {
    PCRE2_SIZE *ov = pcre2_get_ovector_pointer(rx->md);
    size_t st = ov[0], en = ov[1];
    count++;
    pos = (en == st) ? en + 1 : en;
  }
  return count;
}

/* ---- Generated: patterns, input strings and run blocks ---- */
/* ---- Regular expressions of the benchmark, compiled once (RegExpSetup). ---- */
#define NRX 237
static const char RXSRC0[] = "^ba";
static const char RXSRC1[] = "(((\\w+):\\/\\/)([^\\/:]*)(:(\\d+))\?)\?([^#\?]*)(\\\?([^#]*))\?(#(.*))\?";
static const char RXSRC2[] = "^\\s*|\\s*$";
static const char RXSRC3[] = "\\bQBZPbageby_cynprubyqre\\b";
static const char RXSRC4[] = ",";
static const char RXSRC5[] = "\\bQBZPbageby_cynprubyqre\\b";
static const char RXSRC6[] = "^[\\s\\xa0]+|[\\s\\xa0]+$";
static const char RXSRC7[] = "(\\d*)(\\D*)";
static const char RXSRC8[] = "=";
static const char RXSRC9[] = "(^|\\s)lhv\\-h(\\s|$)";
static const char RXSRC10[] = "\\#";
static const char RXSRC11[] = "\\.";
static const char RXSRC12[] = "'";
static const char RXSRC13[] = "\\\?[\\w\\W]*(sevraqvq|punaaryvq|tebhcvq)=([^\\&\\\?#]*)";
static const char RXSRC14[] = "\\s+";
static const char RXSRC15[] = "^\\s*(\\S*(\\s+\\S+)*)\\s*$";
static const char RXSRC16[] = "(-[a-z])";
static const char RXSRC17[] = "(^|[^\\\\])\\\"\\\\\\/Qngr\\((-\?[0-9]+)\\)\\\\\\/\\\"";
static const char RXSRC18[] = "^\\s+|\\s+$";
static const char RXSRC19[] = "(\?:^|\\s+)ba(\?:\\s+|$)";
static const char RXSRC20[] = "[+, ]";
static const char RXSRC21[] = "ybnqrq|pbzcyrgr";
static const char RXSRC22[] = "\\bso_zrah\\b";
static const char RXSRC23[] = "^(\?:(\?:[^:\\/\?#]+):)\?(\?:\\/\\/(\?:[^\\/\?#]*))\?([^\?#]*)(\?:\\\?([^#]*))\?(\?:#(.*))\?";
static const char RXSRC24[] = "uggcf\?:\\/\\/([^\\/]+\\.)\?snprobbx\\.pbz\\/";
static const char RXSRC25[] = "\"";
static const char RXSRC26[] = "^([^\?#]+)(\?:\\\?([^#]*))\?(#.*)\?";
static const char RXSRC27[] = "-\\D";
static const char RXSRC28[] = "\\bnpgvingr\\b";
static const char RXSRC29[] = "%2R";
static const char RXSRC30[] = "%2S";
static const char RXSRC31[] = "^(mu-(PA|GJ)|wn|xb)$";
static const char RXSRC32[] = "\\s\?;\\s\?";
static const char RXSRC33[] = "%\\w\?$";
static const char RXSRC34[] = "TNQP=([^;]*)";
static const char RXSRC35[] = "[<>]";
static const char RXSRC36[] = "uers|fep|fryrpgrq";
static const char RXSRC37[] = "\\s*([+>~\\s])\\s*([a-zA-Z#.*:\\[])";
static const char RXSRC38[] = "^(\\w+|\\*)$";
static const char RXSRC39[] = "\\\\";
static const char RXSRC40[] = " ";
static const char RXSRC41[] = "\\/\\xc4\\/t";
static const char RXSRC42[] = "\\/\\xd6\\/t";
static const char RXSRC43[] = "\\/\\xdc\\/t";
static const char RXSRC44[] = "\\/\\xdf\\/t";
static const char RXSRC45[] = "\\/\\xe4\\/t";
static const char RXSRC46[] = "\\/\\xf6\\/t";
static const char RXSRC47[] = "\\/\\xfc\\/t";
static const char RXSRC48[] = "\\W";
static const char RXSRC49[] = "uers|fep|fglyr";
static const char RXSRC50[] = "(\?:^|\\s+)fryrpgrq(\?:\\s+|$)";
static const char RXSRC51[] = "\\&";
static const char RXSRC52[] = "\\+";
static const char RXSRC53[] = "\\\?";
static const char RXSRC54[] = "\\t";
static const char RXSRC55[] = "(\\$\\{nqiHey\\})|(\\$nqiHey\\b)";
static const char RXSRC56[] = "(\\$\\{cngu\\})|(\\$cngu\\b)";
static const char RXSRC57[] = "##yv4##";
static const char RXSRC58[] = "##yv16##";
static const char RXSRC59[] = "##yv19##";
static const char RXSRC60[] = "(\?:^|\\s+)bss(\?:\\s+|$)";
static const char RXSRC61[] = "^(([^:\\/\?#]+):)\?(\\/\\/([^\\/\?#]*))\?([^\?#]*)(\\\?([^#]*))\?(#(.*))\?$";
static const char RXSRC62[] = "^[^<]*(<(.|\\s)+>)[^>]*$|^#(\\w+)$";
static const char RXSRC63[] = "\\{0\\}";
static const char RXSRC64[] = "\\b[a-z]";
static const char RXSRC65[] = "^uggc:\\/\\/";
static const char RXSRC66[] = "(\?:^|\\s+)qvfnoyrq(\?:\\s+|$)";
static const char RXSRC67[] = "zrah_byq";
static const char RXSRC68[] = "^([#.]\?)((\?:[\\w\\u0128-\\uffff*_-]|\\\\.)*)";
static const char RXSRC69[] = "\\{1\\}";
static const char RXSRC70[] = "\\s+";
static const char RXSRC71[] = "(\\$\\{4\\})|(\\$4\\b)";
static const char RXSRC72[] = "(\\$\\{5\\})|(\\$5\\b)";
static const char RXSRC73[] = "\\{2\\}";
static const char RXSRC74[] = "[^+>] [^+>]";
static const char RXSRC75[] = "\\bucpyv\\s*=\\s*([^;]*)";
static const char RXSRC76[] = "\\bucuvqr\\s*=\\s*([^;]*)";
static const char RXSRC77[] = "\\bucfie\\s*=\\s*([^;]*)";
static const char RXSRC78[] = "\\bhfucjrn\\s*=\\s*([^;]*)";
static const char RXSRC79[] = "\\bmvc\\s*=\\s*([^;]*)";
static const char RXSRC80[] = "^((\?:[\\w\\u0128-\\uffff*_-]|\\\\.)+)(#)((\?:[\\w\\u0128-\\uffff*_-]|\\\\.)+)";
static const char RXSRC81[] = "^([>+~])\\s*(\\w*)";
static const char RXSRC82[] = "^>\\s*((\?:[\\w\\u0128-\\uffff*_-]|\\\\.)+)";
static const char RXSRC83[] = "^[\\s[]\?shapgvba";
static const char RXSRC84[] = "v\\/g.tvs#(.*)";
static const char RXSRC85[] = "eaq_zbqobkva";
static const char RXSRC86[] = ";\\s*";
static const char RXSRC87[] = "(\\$\\{inyhr\\})|(\\$inyhr\\b)";
static const char RXSRC88[] = "(\\$\\{abj\\})|(\\$abj\\b)";
static const char RXSRC89[] = "\\s+$";
static const char RXSRC90[] = "^\\s+";
static const char RXSRC91[] = "(\\\\\\\"|\\x00-|\\x1f|\\x7f-|\\x9f|\\u00ad|\\u0600-|\\u0604|\\u070f|\\u17b4|\\u17b5|\\u200c-|\\"
    "u200f|\\u2028-|\\u202f|\\u2060-|\\u206f|\\ufeff|\\ufff0-|\\uffff)";
static const char RXSRC92[] = "^(:)([\\w-]+)\\(\"\?'\?(.*\?(\\(.*\?\\))\?[^(]*\?)\"\?'\?\\)";
static const char RXSRC93[] = "^([:.#]*)((\?:[\\w\\u0128-\\uffff*_-]|\\\\.)+)";
static const char RXSRC94[] = "^(\\[) *@\?([\\w-]+) *([!*$^~=]*) *('\?\"\?)(.*\?)\\4 *\\]";
static const char RXSRC95[] = "qqqq|qqq|qq|q|ZZZZ|ZZZ|ZZ|Z|llll|ll|l|uu|u|UU|U|zz|z|ff|f|gg|g|sss|ss|s|mmm|mm|m";
static const char RXSRC96[] = "JroXvg\\/(\\S+)";
static const char RXSRC97[] = "\\[";
static const char RXSRC98[] = "(\?:^|;)\\s*sevraqfgre_ynat=([^;]*)";
static const char RXSRC99[] = "([a-zA-Z]|\\s)+";
static const char RXSRC100[] = "(\\s)+e";
static const char RXSRC101[] = ".";
static const char RXSRC102[] = "Fnsnev\\/(\\d+\\.\\d+)";
static const char RXSRC103[] = "d1";
static const char RXSRC104[] = "NQ_VQ";
static const char RXSRC105[] = "d2";
static const char RXSRC106[] = "_";
static const char RXSRC107[] = "(\?:ZFVR.(\\d+\\.\\d+))|(\?:(\?:Sversbk|TenaCnenqvfb|Vprjrnfry).(\\d+\\.\\d+))|(\?:Bcren.(\\d+\\"
    ".\\d+))|(\?:NccyrJroXvg.(\\d+(\?:\\.\\d+)\?))";
static const char RXSRC108[] = "(Znp BF K)|(Jvaqbjf;)";
static const char RXSRC109[] = "Trpxb\\/([0-9]+)";
static const char RXSRC110[] = "\\buvqqra_ryrz\\b";
static const char RXSRC111[] = "\\buvqqra_ryrz\\b";
static const char RXSRC112[] = "puebzr";
static const char RXSRC113[] = "(\?:^|\\s+)gvzrfgnzc(\?:\\s+|$)";
static const char RXSRC114[] = "[A-Za-z]";
static const char RXSRC115[] = "zfvr";
static const char RXSRC116[] = "bcren";
static const char RXSRC117[] = "%3P";
static const char RXSRC118[] = "%3R";
static const char RXSRC119[] = "%3q";
static const char RXSRC120[] = "\\*";
static const char RXSRC121[] = "\\bnpgvir\\b";
static const char RXSRC122[] = "sversbk";
static const char RXSRC123[] = "zfvr";
static const char RXSRC124[] = "bcren";
static const char RXSRC125[] = "\\.([\\w-]+)|\\[(\\w+)(\?:([!*^$~|]\?=)[\"']\?(.*\?)[\"']\?)\?\\]|:([\\w-]+)(\?:\\([\"']\?(.*\?)"
    "\?[\"']\?\\)|$)";
static const char RXSRC126[] = "(\\s+e|\\s+o[0-9]+)";
static const char RXSRC127[] = "<";
static const char RXSRC128[] = ">";
static const char RXSRC129[] = "NccyrJroXvg\\/([^\\s]*)";
static const char RXSRC130[] = "XUGZY";
static const char RXSRC131[] = "(\\$\\{cebg\\})|(\\$cebg\\b)";
static const char RXSRC132[] = "^(.*)\\..*$";
static const char RXSRC133[] = "^.*e(.*)$";
static const char RXSRC134[] = "^.*\\s+(\\S+\\s+\\S+$)";
static const char RXSRC135[] = "(\\$\\{ubfg\\})|(\\$ubfg\\b)";
static const char RXSRC136[] = "(\\$\\{dz\\})|(\\$dz\\b)";
static const char RXSRC137[] = "e";
static const char RXSRC138[] = "##yv0##";
static const char RXSRC139[] = "##\\/o##";
static const char RXSRC140[] = "##\\/v##";
static const char RXSRC141[] = "##\\/h##";
static const char RXSRC142[] = "##o##";
static const char RXSRC143[] = "##oe##";
static const char RXSRC144[] = "##v##";
static const char RXSRC145[] = "##h##";
static const char RXSRC146[] = "##n##";
static const char RXSRC147[] = "##\\/n##";
static const char RXSRC148[] = "#~#argjbexybtb#~#";
static const char RXSRC149[] = " Zbovyr\\/";
static const char RXSRC150[] = "##yv1##";
static const char RXSRC151[] = "##yv10##";
static const char RXSRC152[] = "##yv11##";
static const char RXSRC153[] = "##yv12##";
static const char RXSRC154[] = "##yv13##";
static const char RXSRC155[] = "##yv14##";
static const char RXSRC156[] = "##yv15##";
static const char RXSRC157[] = "##yv17##";
static const char RXSRC158[] = "##yv18##";
static const char RXSRC159[] = "##yv2##";
static const char RXSRC160[] = "##yv20##";
static const char RXSRC161[] = "##yv21##";
static const char RXSRC162[] = "##yv22##";
static const char RXSRC163[] = "##yv23##";
static const char RXSRC164[] = "##yv3##";
static const char RXSRC165[] = "##yv5##";
static const char RXSRC166[] = "##yv6##";
static const char RXSRC167[] = "##yv7##";
static const char RXSRC168[] = "##yv8##";
static const char RXSRC169[] = "##yv9##";
static const char RXSRC170[] = "AbxvnA[^\\/]*";
static const char RXSRC171[] = "(\?:^|\\s+)bss(\?:\\s+|$)";
static const char RXSRC172[] = "(\\$\\{0\\})|(\\$0\\b)";
static const char RXSRC173[] = "(\\$\\{1\\})|(\\$1\\b)";
static const char RXSRC174[] = "(\\$\\{pbzcyrgr\\})|(\\$pbzcyrgr\\b)";
static const char RXSRC175[] = "(\\$\\{sentzrag\\})|(\\$sentzrag\\b)";
static const char RXSRC176[] = "(\\$\\{ubfgcbeg\\})|(\\$ubfgcbeg\\b)";
static const char RXSRC177[] = "(\\$\\{cebgbpby\\})|(\\$cebgbpby\\b)";
static const char RXSRC178[] = "(\\$\\{dhrel\\})|(\\$dhrel\\b)";
static const char RXSRC179[] = "(\\$\\{2\\})|(\\$2\\b)";
static const char RXSRC180[] = "(\\$\\{3\\})|(\\$3\\b)";
static const char RXSRC181[] = " PC=i=(\\d+)&oe=(.)";
static const char RXSRC182[] = "Fnsnev|Xbadhrebe|XUGZY";
static const char RXSRC183[] = "(^|.)(ronl|qri-ehf3.wbg)(|fgberf|zbgbef|yvirnhpgvbaf|jvxv|rkcerff|punggre).(pbz(|.nh|.pa|.ux|.zl"
    "|.ft|.oe|.zk)|pb(.hx|.xe|.am)|pn|qr|se|vg|ay|or|ng|pu|vr|va|rf|cy|cu|fr)$";
static const char RXSRC184[] = "\\d+";
static const char RXSRC185[] = "[A-Z]";
static const char RXSRC186[] = "#([\\w-]+)";
static const char RXSRC187[] = "NqborNVE\\/([^\\s]*)";
static const char RXSRC188[] = "NccyrJroXvg\\/([^ ]*)";
static const char RXSRC189[] = "XUGZY";
static const char RXSRC190[] = "^(\?:obql|ugzy)$";
static const char RXSRC191[] = "^(\\w+|\\*)";
static const char RXSRC192[] = "znp|jva|yvahk";
static const char RXSRC193[] = "eton\?\\([\\d\\s,]+\\)";
static const char RXSRC194[] = "\\r";
static const char RXSRC195[] = "((ZFVR\\s+([6-9]|\\d\\d)\\.))";
static const char RXSRC196[] = "Xbadhrebe|Fnsnev|XUGZY";
static const char RXSRC197[] = "&";
static const char RXSRC198[] = "(\\$\\{anzr\\})|(\\$anzr\\b)";
static const char RXSRC199[] = "(..-HF)(\\|(\\d+)|)";
static const char RXSRC200[] = "ZFVR\\s+5[.]01";
static const char RXSRC201[] = "HF(\?=;)";
static const char RXSRC202[] = "\\bp:(..)";
static const char RXSRC203[] = "\\bhfucce\\s*=\\s*([^;]*)";
static const char RXSRC204[] = "\\bjci\\s*=\\s*([^;]*)";
static const char RXSRC205[] = "\\|p:([a-z]{2})";
static const char RXSRC206[] = "^Fubpxjnir Synfu (\\d)";
static const char RXSRC207[] = "^Fubpxjnir Synfu (\\d+)";
static const char RXSRC208[] = "jroxvg";
static const char RXSRC209[] = "^erzbgr_vsenzr_";
static const char RXSRC210[] = "\\-";
static const char RXSRC211[] = "^$";
static const char RXSRC212[] = "^znk\\d+$";
static const char RXSRC213[] = "^zva\\d+$";
static const char RXSRC214[] = "^erfgber$";
static const char RXSRC215[] = "frpgvba\\d+_pbagragf";
static const char RXSRC216[] = "^";
static const char RXSRC217[] = "^.*\\.(.*)\\s.*$";
static const char RXSRC218[] = "^.*jroxvg\\/(\\d+(\\.\\d+)\?).*$";
static const char RXSRC219[] = "\\b\\w+\\b";
static const char RXSRC220[] = "\\/";
static const char RXSRC221[] = "#[a-z0-9]+$";
static const char RXSRC222[] = "(\?:^|\\s+)lhv-ani(\?:\\s+|$)";
static const char RXSRC223[] = "(\?:^|\\s+)lhv-anifrg(\?:\\s+|$)";
static const char RXSRC224[] = "(\?:^|\\s+)lhv-anifrg-gbc(\?:\\s+|$)";
static const char RXSRC225[] = "(pbzcngvoyr|jroxvg)";
static const char RXSRC226[] = ".+(\?:ei|vg|en|vr)[\\/: ]([\\d.]+)";
static const char RXSRC227[] = "\\/onfr.wf(\\\?.+)\?$";
static const char RXSRC228[] = "xbadhrebe";
static const char RXSRC229[] = "znp";
static const char RXSRC230[] = "zbmvyyn";
static const char RXSRC231[] = "zfvr";
static const char RXSRC232[] = "ag\\s5\\.1";
static const char RXSRC233[] = "bcren";
static const char RXSRC234[] = "fnsnev";
static const char RXSRC235[] = "jva";
static const char RXSRC236[] = "jvaqbjf";
static const char *const RX_SRC[NRX] = {RXSRC0, RXSRC1, RXSRC2, RXSRC3, RXSRC4, RXSRC5, RXSRC6, RXSRC7, RXSRC8, RXSRC9, RXSRC10, RXSRC11, RXSRC12, RXSRC13, RXSRC14, RXSRC15, RXSRC16, RXSRC17, RXSRC18, RXSRC19, RXSRC20, RXSRC21, RXSRC22, RXSRC23, RXSRC24, RXSRC25, RXSRC26, RXSRC27, RXSRC28, RXSRC29, RXSRC30, RXSRC31, RXSRC32, RXSRC33, RXSRC34, RXSRC35, RXSRC36, RXSRC37, RXSRC38, RXSRC39, RXSRC40, RXSRC41, RXSRC42, RXSRC43, RXSRC44, RXSRC45, RXSRC46, RXSRC47, RXSRC48, RXSRC49, RXSRC50, RXSRC51, RXSRC52, RXSRC53, RXSRC54, RXSRC55, RXSRC56, RXSRC57, RXSRC58, RXSRC59, RXSRC60, RXSRC61, RXSRC62, RXSRC63, RXSRC64, RXSRC65, RXSRC66, RXSRC67, RXSRC68, RXSRC69, RXSRC70, RXSRC71, RXSRC72, RXSRC73, RXSRC74, RXSRC75, RXSRC76, RXSRC77, RXSRC78, RXSRC79, RXSRC80, RXSRC81, RXSRC82, RXSRC83, RXSRC84, RXSRC85, RXSRC86, RXSRC87, RXSRC88, RXSRC89, RXSRC90, RXSRC91, RXSRC92, RXSRC93, RXSRC94, RXSRC95, RXSRC96, RXSRC97, RXSRC98, RXSRC99, RXSRC100, RXSRC101, RXSRC102, RXSRC103, RXSRC104, RXSRC105, RXSRC106, RXSRC107, RXSRC108, RXSRC109, RXSRC110, RXSRC111, RXSRC112, RXSRC113, RXSRC114, RXSRC115, RXSRC116, RXSRC117, RXSRC118, RXSRC119, RXSRC120, RXSRC121, RXSRC122, RXSRC123, RXSRC124, RXSRC125, RXSRC126, RXSRC127, RXSRC128, RXSRC129, RXSRC130, RXSRC131, RXSRC132, RXSRC133, RXSRC134, RXSRC135, RXSRC136, RXSRC137, RXSRC138, RXSRC139, RXSRC140, RXSRC141, RXSRC142, RXSRC143, RXSRC144, RXSRC145, RXSRC146, RXSRC147, RXSRC148, RXSRC149, RXSRC150, RXSRC151, RXSRC152, RXSRC153, RXSRC154, RXSRC155, RXSRC156, RXSRC157, RXSRC158, RXSRC159, RXSRC160, RXSRC161, RXSRC162, RXSRC163, RXSRC164, RXSRC165, RXSRC166, RXSRC167, RXSRC168, RXSRC169, RXSRC170, RXSRC171, RXSRC172, RXSRC173, RXSRC174, RXSRC175, RXSRC176, RXSRC177, RXSRC178, RXSRC179, RXSRC180, RXSRC181, RXSRC182, RXSRC183, RXSRC184, RXSRC185, RXSRC186, RXSRC187, RXSRC188, RXSRC189, RXSRC190, RXSRC191, RXSRC192, RXSRC193, RXSRC194, RXSRC195, RXSRC196, RXSRC197, RXSRC198, RXSRC199, RXSRC200, RXSRC201, RXSRC202, RXSRC203, RXSRC204, RXSRC205, RXSRC206, RXSRC207, RXSRC208, RXSRC209, RXSRC210, RXSRC211, RXSRC212, RXSRC213, RXSRC214, RXSRC215, RXSRC216, RXSRC217, RXSRC218, RXSRC219, RXSRC220, RXSRC221, RXSRC222, RXSRC223, RXSRC224, RXSRC225, RXSRC226, RXSRC227, RXSRC228, RXSRC229, RXSRC230, RXSRC231, RXSRC232, RXSRC233, RXSRC234, RXSRC235, RXSRC236};
static const char *const RX_FLAGS[NRX] = {"", "", "g", "", "", "g", "g", "g", "", "", "g", "g", "g", "i", "g", "", "i", "g", "g", "", "", "", "", "", "", "g", "", "g", "", "gi", "gi", "", "", "", "i", "g", "", "g", "", "g", "g", "", "", "", "", "", "", "", "g", "", "", "g", "g", "g", "g", "g", "g", "gi", "gi", "gi", "", "", "", "g", "g", "", "", "g", "", "g", "", "g", "g", "g", "", "i", "i", "i", "i", "i", "", "i", "", "", "i", "", "", "g", "g", "", "", "g", "", "", "", "g", "", "g", "", "", "", "", "", "g", "g", "g", "g", "", "", "", "g", "", "i", "", "g", "gi", "gi", "g", "g", "g", "g", "", "i", "i", "i", "g", "", "g", "g", "", "", "g", "", "", "", "g", "g", "", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "g", "", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "gi", "", "g", "g", "g", "g", "g", "g", "g", "g", "g", "g", "", "gi", "i", "g", "g", "", "", "", "gi", "i", "", "i", "", "g", "", "", "g", "g", "i", "", "i", "i", "i", "i", "i", "", "", "", "", "g", "", "", "", "", "", "", "", "", "g", "g", "i", "", "", "", "", "", "", "", "", "", "", "", "", "", "", ""};
static Rx *RX[NRX];
static void setup_regexps(void) {
  for (int i = 0; i < NRX; i++) RX[i] = rx_compile(RX_SRC[i], strlen(RX_SRC[i]), RX_FLAGS[i]);
}

/* ---- Strings and variant arrays (the file-level var statements). ---- */
static Str str0;
static Str str1;
static Str str10;
static Str str11;
static Str str12;
static Str str13;
static Str str14;
static Str str15;
static Str str16;
static Str str17;
static Str str18;
static Str str19;
static Str str2;
static Str str20;
static Str str21;
static Str str22;
static Str str23;
static Str str24;
static Str str25;
static Str str26;
static Str str27;
static Str str28;
static Str str29;
static Str str3;
static Str str30;
static Str str31;
static Str str32;
static Str str33;
static Str str34;
static Str str35;
static Str str36;
static Str str37;
static Str str38;
static Str str39;
static Str str4;
static Str str40;
static Str str41;
static Str str42;
static Str str43;
static Str str44;
static Str str45;
static Str str46;
static Str str47;
static Str str48;
static Str str49;
static Str str5;
static Str str50;
static Str str51;
static Str str52;
static Str str53;
static Str str54;
static Str str55;
static Str str56;
static Str str57;
static Str str58;
static Str str59;
static Str str6;
static Str str60;
static Str str61;
static Str str62;
static Str str63;
static Str str64;
static Str str65;
static Str str66;
static Str str67;
static Str str68;
static Str str69;
static Str str7;
static Str str70;
static Str str71;
static Str str72;
static Str str73;
static Str str74;
static Str str75;
static Str str76;
static Str str77;
static Str str78;
static Str str79;
static Str str8;
static Str str80;
static Str str81;
static Str str82;
static Str str83;
static Str str84;
static Str str85;
static Str str86;
static Str str87;
static Str str88;
static Str str89;
static Str str9;
static Str str90;
static Str str91;
static Str str92;
static Str str93;
static Str str94;
static Str str95;
static Str str96;
static Str str97;
static Str str98;
static Str *s0;
static Str *s1;
static Str *s10;
static Str *s100;
static Str *s101;
static Str *s102;
static Str *s103;
static Str *s11;
static Str *s12;
static Str *s13;
static Str *s14;
static Str *s15;
static Str *s16;
static Str *s17;
static Str *s18;
static Str *s19;
static Str *s2;
static Str *s20;
static Str *s21;
static Str *s22;
static Str *s23;
static Str *s24;
static Str *s25;
static Str *s26;
static Str *s27;
static Str *s28;
static Str *s29;
static Str *s3;
static Str *s30;
static Str *s31;
static Str *s32;
static Str *s33;
static Str *s34;
static Str *s35;
static Str *s36;
static Str *s37;
static Str *s38;
static Str *s39;
static Str *s4;
static Str *s41;
static Str *s42;
static Str *s43;
static Str *s44;
static Str *s45;
static Str *s46;
static Str *s47;
static Str *s48;
static Str *s49;
static Str *s5;
static Str *s50;
static Str *s51;
static Str *s52;
static Str *s53;
static Str *s54;
static Str *s55;
static Str *s56;
static Str *s57;
static Str *s57a;
static Str *s58;
static Str *s59;
static Str *s6;
static Str *s60;
static Str *s61;
static Str *s62;
static Str *s63;
static Str *s64;
static Str *s65;
static Str *s66;
static Str *s67;
static Str *s68;
static Str *s69;
static Str *s7;
static Str *s70;
static Str *s71;
static Str *s72;
static Str *s73;
static Str *s74;
static Str *s75;
static Str *s76;
static Str *s77;
static Str *s78;
static Str *s79;
static Str *s8;
static Str *s80;
static Str *s81;
static Str *s82;
static Str *s83;
static Str *s84;
static Str *s85;
static Str *s86;
static Str *s87;
static Str *s88;
static Str *s89;
static Str *s9;
static Str *s90;
static Str *s91;
static Str *s92;
static Str *s93;
static Str *s94;
static Str *s95;
static Str *s96;
static Str *s97;
static Str *s98;
static Str *s99;

static void setup_vars(void) {
  str0 = (Str){"Zbmvyyn/5.0 (Jvaqbjf; H; Jvaqbjf AG 5.1; ra-HF) NccyrJroXvg/528.9 (XUGZY, yvxr Trpxb) Puebzr/2.0"
    ".157.0 Fnsnev/528.9", 115};
  str1 = (Str){"Fubpxjnir Synfu 9.0  e115", 25};
  s0 = compute_variants((Str){"pyvpx", 5}, 6511);
  s1 = compute_variants((Str){"uggc://jjj.snprobbx.pbz/ybtva.cuc", 33}, 1844);
  s2 = compute_variants((Str){"QBZPbageby_cynprubyqre", 22}, 739);
  s3 = compute_variants((Str){"uggc://jjj.snprobbx.pbz/", 24}, 598);
  s4 = compute_variants((Str){"uggc://jjj.snprobbx.pbz/fepu.cuc", 32}, 454);
  s5 = compute_variants((Str){"qqqq, ZZZ q, llll", 17}, 352);
  s6 = compute_variants((Str){"vachggrkg QBZPbageby_cynprubyqre", 32}, 312);
  s7 = compute_variants((Str){"/ZlFcnprUbzrcntr/Vaqrk-FvgrUbzr,10000000", 40}, 282);
  s8 = compute_variants((Str){"vachggrkg", 9}, 177);
  s9 = compute_variants((Str){"528.9", 5}, 170);
  s10 = compute_variants((Str){"528", 3}, 170);
  s11 = compute_variants((Str){"VCPhygher=ra-HF", 15}, 156);
  s12 = compute_variants((Str){"CersreerqPhygher=ra-HF", 22}, 156);
  s13 = compute_variants((Str){"xrlcerff", 8}, 144);
  s14 = compute_variants((Str){"521", 3}, 139);
  s15 = compute_variants(str0, 139);
  s16 = compute_variants((Str){"qvi .so_zrah", 12}, 137);
  s17 = compute_variants((Str){"qvi.so_zrah", 11}, 137);
  s18 = compute_variants((Str){"uvqqra_ryrz", 11}, 117);
  s19 = compute_variants((Str){"sevraqfgre_naba=nvq%3Qn6ss9p85n868ro9s059pn854735956o3%26ers%3Q%26df%3Q%26vpgl%3QHF", 83}, 95);
  s20 = compute_variants((Str){"uggc://ubzr.zlfcnpr.pbz/vaqrk.psz", 33}, 93);
  s21 = compute_variants(str1, 92);
  s22 = compute_variants((Str){"svefg", 5}, 85);
  s23 = compute_variants((Str){"uggc://cebsvyr.zlfcnpr.pbz/vaqrk.psz", 36}, 85);
  s24 = compute_variants((Str){"ynfg", 4}, 85);
  s25 = compute_variants((Str){"qvfcynl", 7}, 85);
  str2 = (Str){"{\"anzr\":\"\",\"ahzoreSbezng\":{\"PheeraplQrpvznyQvtvgf\":2,\"PheeraplQrpvznyFrcnengbe\":\".\","
    "\"VfErnqBayl\":gehr,\"PheeraplTebhcFvmrf\":[3],\"AhzoreTebhcFvmrf\":[3],\"CrepragTebhcFvmrf\":[3"
    "],\"PheeraplTebhcFrcnengbe\":\",\",\"PheeraplFlzoby\":\"\302\244\",\"AnAFlzoby\":\"AnA\",\"PheeraplA"
    "rtngvirCnggrea\":0,\"AhzoreArtngvirCnggrea\":1,\"CrepragCbfvgvirCnggrea\":0,\"CrepragArtngvirCng"
    "grea\":0,\"ArtngvirVasvavglFlzoby\":\"-Vasvavgl\",\"ArtngvirFvta\":\"-\",\"AhzoreQrpvznyQvtvgf\""
    ":2,\"AhzoreQrpvznyFrcnengbe\":\".\",\"AhzoreTebhcFrcnengbe\":\",\",\"PheeraplCbfvgvirCnggrea\":0"
    ",\"CbfvgvirVasvavglFlzoby\":\"Vasvavgl\",\"CbfvgvirFvta\":\"+\",\"CrepragQrpvznyQvtvgf\":2,\"Cre"
    "pragQrpvznyFrcnengbe\":\".\",\"CrepragTebhcFrcnengbe\":\",\",\"CrepragFlzoby\":\"%\",\"CreZvyyrF"
    "lzoby\":\"\302\211\",\"AngvirQvtvgf\":[\"0\",\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\""
    ",\"9\"],\"QvtvgFhofgvghgvba\":1},\"qngrGvzrSbezng\":{\"NZQrfvtangbe\":\"NZ\",\"Pnyraqne\":{\"Zva"
    "FhccbegrqQngrGvzr\":\"@-62135568000000@\",\"ZnkFhccbegrqQngrGvzr\":\"@253402300799999@\",\"Nytbe"
    "vguzGlcr\":1,\"PnyraqneGlcr\":1,\"Renf\":[1],\"GjbQvtvgLrneZnk\":2029,\"VfErnqBayl\":gehr},\"Qng"
    "rFrcnengbe\":\"/\",\"SvefgQnlBsJrrx\":0,\"PnyraqneJrrxEhyr\":0,\"ShyyQngrGvzrCnggrea\":\"qqqq, q"
    "q ZZZZ llll UU:zz:ff\",\"YbatQngrCnggrea\":\"qqqq, qq ZZZZ llll\",\"YbatGvzrCnggrea\":\"UU:zz:ff"
    "\",\"ZbaguQnlCnggrea\":\"ZZZZ qq\",\"CZQrfvtangbe\":\"CZ\",\"ESP1123Cnggrea\":\"qqq, qq ZZZ llll"
    " UU':'zz':'ff 'TZG'\",\"FubegQngrCnggrea\":\"ZZ/qq/llll\",\"FubegGvzrCnggrea\":\"UU:zz\",\"Fbegn"
    "oyrQngrGvzrCnggrea\":\"llll'-'ZZ'-'qq'G'UU':'zz':'ff\",\"GvzrFrcnengbe\":\":\",\"HavirefnyFbegno"
    "yrQngrGvzrCnggrea\":\"llll'-'ZZ'-'qq UU':'zz':'ff'M'\",\"LrneZbaguCnggrea\":\"llll ZZZZ\",\"Nooe"
    "rivngrqQnlAnzrf\":[\"Fha\",\"Zba\",\"Ghr\",\"Jrq\",\"Guh\",\"Sev\",\"Fng\"],\"FubegrfgQnlAnzrf\""
    ":[\"Fh\",\"Zb\",\"Gh\",\"Jr\",\"Gu\",\"Se\",\"Fn\"],\"QnlAnzrf\":[\"Fhaqnl\",\"Zbaqnl\",\"Ghrfqn"
    "l\",\"Jrqarfqnl\",\"Guhefqnl\",\"Sevqnl\",\"Fngheqnl\"],\"NooerivngrqZbaguAnzrf\":[\"Wna\",\"Sro"
    "\",\"Zne\",\"Nce\",\"Znl\",\"Wha\",\"Why\",\"Nht\",\"Frc\",\"Bpg\",\"Abi\",\"Qrp\",\"\"],\"Zbagu"
    "Anzrf\":[\"Wnahnel\",\"Sroehnel\",\"Znepu\",\"Ncevy\",\"Znl\",\"Whar\",\"Whyl\",\"Nhthfg\",\"Frc"
    "grzore\",\"Bpgbore\",\"Abirzore\",\"Qrprzore\",\"\"],\"VfErnqBayl\":gehr,\"AngvirPnyraqneAnzr\":"
    "\"Tertbevna Pnyraqne\",\"NooerivngrqZbaguTravgvirAnzrf\":[\"Wna\",\"Sro\",\"Zne\",\"Nce\",\"Znl"
    "\",\"Wha\",\"Why\",\"Nht\",\"Frc\",\"Bpg\",\"Abi\",\"Qrp\",\"\"],\"ZbaguTravgvirAnzrf\":[\"Wnahn"
    "el\",\"Sroehnel\",\"Znepu\",\"Ncevy\",\"Znl\",\"Whar\",\"Whyl\",\"Nhthfg\",\"Frcgrzore\",\"Bpgbo"
    "re\",\"Abirzore\",\"Qrprzore\",\"\"]}}", 2259};
  str3 = (Str){"{\"anzr\":\"ra-HF\",\"ahzoreSbezng\":{\"PheeraplQrpvznyQvtvgf\":2,\"PheeraplQrpvznyFrcnengbe\":"
    "\".\",\"VfErnqBayl\":snyfr,\"PheeraplTebhcFvmrf\":[3],\"AhzoreTebhcFvmrf\":[3],\"CrepragTebhcFvm"
    "rf\":[3],\"PheeraplTebhcFrcnengbe\":\",\",\"PheeraplFlzoby\":\"$\",\"AnAFlzoby\":\"AnA\",\"Pheer"
    "aplArtngvirCnggrea\":0,\"AhzoreArtngvirCnggrea\":1,\"CrepragCbfvgvirCnggrea\":0,\"CrepragArtngvi"
    "rCnggrea\":0,\"ArtngvirVasvavglFlzoby\":\"-Vasvavgl\",\"ArtngvirFvta\":\"-\",\"AhzoreQrpvznyQvtv"
    "gf\":2,\"AhzoreQrpvznyFrcnengbe\":\".\",\"AhzoreTebhcFrcnengbe\":\",\",\"PheeraplCbfvgvirCnggrea"
    "\":0,\"CbfvgvirVasvavglFlzoby\":\"Vasvavgl\",\"CbfvgvirFvta\":\"+\",\"CrepragQrpvznyQvtvgf\":2,"
    "\"CrepragQrpvznyFrcnengbe\":\".\",\"CrepragTebhcFrcnengbe\":\",\",\"CrepragFlzoby\":\"%\",\"CreZ"
    "vyyrFlzoby\":\"\302\211\",\"AngvirQvtvgf\":[\"0\",\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\","
    "\"8\",\"9\"],\"QvtvgFhofgvghgvba\":1},\"qngrGvzrSbezng\":{\"NZQrfvtangbe\":\"NZ\",\"Pnyraqne\":{"
    "\"ZvaFhccbegrqQngrGvzr\":\"@-62135568000000@\",\"ZnkFhccbegrqQngrGvzr\":\"@253402300799999@\",\""
    "NytbevguzGlcr\":1,\"PnyraqneGlcr\":1,\"Renf\":[1],\"GjbQvtvgLrneZnk\":2029,\"VfErnqBayl\":snyfr}"
    ",\"QngrFrcnengbe\":\"/\",\"SvefgQnlBsJrrx\":0,\"PnyraqneJrrxEhyr\":0,\"ShyyQngrGvzrCnggrea\":\"q"
    "qqq, ZZZZ qq, llll u:zz:ff gg\",\"YbatQngrCnggrea\":\"qqqq, ZZZZ qq, llll\",\"YbatGvzrCnggrea\":"
    "\"u:zz:ff gg\",\"ZbaguQnlCnggrea\":\"ZZZZ qq\",\"CZQrfvtangbe\":\"CZ\",\"ESP1123Cnggrea\":\"qqq,"
    " qq ZZZ llll UU':'zz':'ff 'TZG'\",\"FubegQngrCnggrea\":\"Z/q/llll\",\"FubegGvzrCnggrea\":\"u:zz "
    "gg\",\"FbegnoyrQngrGvzrCnggrea\":\"llll'-'ZZ'-'qq'G'UU':'zz':'ff\",\"GvzrFrcnengbe\":\":\",\"Hav"
    "irefnyFbegnoyrQngrGvzrCnggrea\":\"llll'-'ZZ'-'qq UU':'zz':'ff'M'\",\"LrneZbaguCnggrea\":\"ZZZZ, "
    "llll\",\"NooerivngrqQnlAnzrf\":[\"Fha\",\"Zba\",\"Ghr\",\"Jrq\",\"Guh\",\"Sev\",\"Fng\"],\"Fubeg"
    "rfgQnlAnzrf\":[\"Fh\",\"Zb\",\"Gh\",\"Jr\",\"Gu\",\"Se\",\"Fn\"],\"QnlAnzrf\":[\"Fhaqnl\",\"Zbaq"
    "nl\",\"Ghrfqnl\",\"Jrqarfqnl\",\"Guhefqnl\",\"Sevqnl\",\"Fngheqnl\"],\"NooerivngrqZbaguAnzrf\":["
    "\"Wna\",\"Sro\",\"Zne\",\"Nce\",\"Znl\",\"Wha\",\"Why\",\"Nht\",\"Frc\",\"Bpg\",\"Abi\",\"Qrp\","
    "\"\"],\"ZbaguAnzrf\":[\"Wnahnel\",\"Sroehnel\",\"Znepu\",\"Ncevy\",\"Znl\",\"Whar\",\"Whyl\",\"N"
    "hthfg\",\"Frcgrzore\",\"Bpgbore\",\"Abirzore\",\"Qrprzore\",\"\"],\"VfErnqBayl\":snyfr,\"AngvirP"
    "nyraqneAnzr\":\"Tertbevna Pnyraqne\",\"NooerivngrqZbaguTravgvirAnzrf\":[\"Wna\",\"Sro\",\"Zne\","
    "\"Nce\",\"Znl\",\"Wha\",\"Why\",\"Nht\",\"Frc\",\"Bpg\",\"Abi\",\"Qrp\",\"\"],\"ZbaguTravgvirAnz"
    "rf\":[\"Wnahnel\",\"Sroehnel\",\"Znepu\",\"Ncevy\",\"Znl\",\"Whar\",\"Whyl\",\"Nhthfg\",\"Frcgrz"
    "ore\",\"Bpgbore\",\"Abirzore\",\"Qrprzore\",\"\"]}}", 2273};
  str4 = (Str){"HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH"
    "3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmW"
    "yWyM2yiox5uoJH9D0R%3Q", 213};
  str5 = (Str){"HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH"
    "3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmW"
    "yWyM2yiox5uoJH9D0R=", 211};
  str6 = (Str){"uggc://jjj.snprobbx.pbz/vaqrk.cuc", 33};
  str7 = (Str){";;jvaqbj.IjPurpxZbhfrCbfvgvbaNQ_VQ=shapgvba(r){vs(!r)ine r=jvaqbj.rirag;ine c=-1;vs(d1)c=d1.Ebyy"
    "bssCnary;ine bo=IjTrgBow(\"IjCnayNQ_VQ_\"+c);vs(bo&&bo.fglyr.ivfvovyvgl==\"ivfvoyr\"){ine fns=Ij"
    "Fns\?8:0;ine pheK=r.pyvragK+IjBOFpe(\"U\")+fns,pheL=r.pyvragL+IjBOFpe(\"I\")+fns;ine y=IjBOEC(NQ"
    "_VQ,bo,\"Y\"),g=IjBOEC(NQ_VQ,bo,\"G\");ine e=y+d1.Cnaryf[c].Jvqgu,o=g+d1.Cnaryf[c].Urvtug;vs((ph"
    "eK<y)||(pheK>e)||(pheL<g)||(pheL>o)){vs(jvaqbj.IjBaEbyybssNQ_VQ)IjBaEbyybssNQ_VQ(c);ryfr IjPybfr"
    "Nq(NQ_VQ,c,gehr,\"\");}ryfr erghea;}IjPnapryZbhfrYvfgrareNQ_VQ();};;jvaqbj.IjFrgEbyybssCnaryNQ_V"
    "Q=shapgvba(c){ine z=\"zbhfrzbir\",q=qbphzrag,s=IjPurpxZbhfrCbfvgvbaNQ_VQ;c=IjTc(NQ_VQ,c);vs(d1&&"
    "d1.EbyybssCnary>-1)IjPnapryZbhfrYvfgrareNQ_VQ();vs(d1)d1.EbyybssCnary=c;gel{vs(q.nqqRiragYvfgrar"
    "e)q.nqqRiragYvfgrare(z,s,snyfr);ryfr vs(q.nggnpuRirag)q.nggnpuRirag(\"ba\"+z,s);}pngpu(r){}};;jv"
    "aqbj.IjPnapryZbhfrYvfgrareNQ_VQ=shapgvba(){ine z=\"zbhfrzbir\",q=qbphzrag,s=IjPurpxZbhfrCbfvgvba"
    "NQ_VQ;vs(d1)d1.EbyybssCnary=-1;gel{vs(q.erzbirRiragYvfgrare)q.erzbirRiragYvfgrare(z,s,snyfr);ryf"
    "r vs(q.qrgnpuRirag)q.qrgnpuRirag(\"ba\"+z,s);}pngpu(r){}};;d1.IjTc=d2(n,c){ine nq=d1;vs(vfAnA(c)"
    "){sbe(ine v=0;v<nq.Cnaryf.yratgu;v++)vs(nq.Cnaryf[v].Anzr==c)erghea v;erghea 0;}erghea c;};;d1.I"
    "jTpy=d2(n,c,p){ine cn=d1.Cnaryf[IjTc(n,c)];vs(!cn)erghea 0;vs(vfAnA(p)){sbe(ine v=0;v<cn.Pyvpxgu"
    "ehf.yratgu;v++)vs(cn.Pyvpxguehf[v].Anzr==p)erghea v;erghea 0;}erghea p;};;d1.IjGenpr=d2(n,f){gel"
    "{vs(jvaqbj[\"Ij\"+\"QtQ\"])jvaqbj[\"Ij\"+\"QtQ\"](n,1,f);}pngpu(r){}};;d1.IjYvzvg1=d2(n,f){ine n"
    "q=d1,vh=f.fcyvg(\"/\");sbe(ine v=0,p=0;v<vh.yratgu;v++){vs(vh[v].yratgu>0){vs(nq.FzV.yratgu>0)nq"
    ".FzV+=\"/\";nq.FzV+=vh[v];nq.FtZ[nq.FtZ.yratgu]=snyfr;}}};;d1.IjYvzvg0=d2(n,f){ine nq=d1,vh=f.fc"
    "yvg(\"/\");sbe(ine v=0;v<vh.yratgu;v++){vs(vh[v].yratgu>0){vs(nq.OvC.yratgu>0)nq.OvC+=\"/\";nq.O"
    "vC+=vh[v];}}};;d1.IjRVST=d2(n,c){jvaqbj[\"IjCnayNQ_VQ_\"+c+\"_Bow\"]=IjTrgBow(\"IjCnayNQ_VQ_\"+c"
    "+\"_Bow\");vs(jvaqbj[\"IjCnayNQ_VQ_\"+c+\"_Bow\"]==ahyy)frgGvzrbhg(\"IjRVST(NQ_VQ,\"+c+\")\",d1."
    "rvsg);};;d1.IjNavzSHC=d2(n,c){ine nq=d1;vs(c>nq.Cnaryf.yratgu)erghea;ine cna=nq.Cnaryf[c],nn=geh"
    "r,on=gehr,yn=gehr,en=gehr,cn=nq.Cnaryf[0],sf=nq.ShF,j=cn.Jvqgu,u=cn.Urvtug;vs(j==\"100%\"){j=sf;"
    "en=snyfr;yn=snyfr;}vs(u==\"100%\"){u=sf;nn=snyfr;on=snyfr;}vs(cn.YnY==\"Y\")yn=snyfr;vs(cn.YnY=="
    "\"E\")en=snyfr;vs(cn.GnY==\"G\")nn=snyfr;vs(cn.GnY==\"O\")on=snyfr;ine k=0,l=0;fjvgpu(nq.NshP%8)"
    "{pnfr 0:oernx;pnfr 1:vs(nn)l=-sf;oernx;pnfr 2:k=j-sf;oernx;pnfr 3:vs(en)k=j;oernx;pnfr 4:k=j-sf;"
    "l=u-sf;oernx;pnfr 5:k=j-sf;vs(on)l=u;oernx;pnfr 6:l=u-sf;oernx;pnfr 7:vs(yn)k=-sf;l=u-sf;oernx;}"
    "vs(nq.NshP++ <nq.NshG)frgGvzrbhg((\"IjNavzSHC(NQ_VQ,\"+c+\")\"),nq.NshC);ryfr{k=-1000;l=k;}cna.Y"
    "rsgBssfrg=k;cna.GbcBssfrg=l;IjNhErcb(n,c);};;d1.IjTrgErnyCbfvgvba=d2(n,b,j){erghea IjBOEC.nccyl("
    "guvf,nethzragf);};;d1.IjPnapryGvzrbhg=d2(n,c){c=IjTc(n,c);ine cay=d1.Cnaryf[c];vs(cay&&cay.UgU!="
    "\"\"){pyrneGvzrbhg(cay.UgU);}};;d1.IjPnapryNyyGvzrbhgf=d2(n){vs(d1.YbpxGvzrbhgPunatrf)erghea;sbe"
    "(ine c=0;c<d1.bac;c++)IjPnapryGvzrbhg(n,c);};;d1.IjFgnegGvzrbhg=d2(n,c,bG){c=IjTc(n,c);ine cay=d"
    "1.Cnaryf[c];vs(cay&&((cay.UvqrGvzrbhgInyhr>0)||(nethzragf.yratgu==3&&bG>0))){pyrneGvzrbhg(cay.Ug"
    "U);cay.UgU=frgGvzrbhg(cay.UvqrNpgvba,(nethzragf.yratgu==3\?bG:cay.UvqrGvzrbhgInyhr));}};;d1.IjEr"
    "frgGvzrbhg=d2(n,c,bG){c=IjTc(n,c);IjPnapryGvzrbhg(n,c);riny(\"IjFgnegGvzrbhg(NQ_VQ,c\"+(nethzrag"
    "f.yratgu==3\?\",bG\":\"\")+\")\");};;d1.IjErfrgNyyGvzrbhgf=d2(n){sbe(ine c=0;c<d1.bac;c++)IjErfr"
    "gGvzrbhg(n,c);};;d1.IjQrgnpure=d2(n,rig,sap){gel{vs(IjQVR5)riny(\"jvaqbj.qrgnpuRirag('ba\"+rig+"
    "\"',\"+sap+\"NQ_VQ)\");ryfr vs(!IjQVRZnp)riny(\"jvaqbj.erzbirRiragYvfgrare('\"+rig+\"',\"+sap+\""
    "NQ_VQ,snyfr)\");}pngpu(r){}};;d1.IjPyrnaHc=d2(n){IjCvat(n,\"G\");ine nq=d1;sbe(ine v=0;v<nq.Cnar"
    "yf.yratgu;v++){IjUvqrCnary(n,v,gehr);}gel{IjTrgBow(nq.gya).vaareUGZY=\"\";}pngpu(r){}vs(nq.gya!="
    "nq.gya2)gel{IjTrgBow(nq.gya2).vaareUGZY=\"\";}pngpu(r){}gel{d1=ahyy;}pngpu(r){}gel{IjQrgnpure(n,"
    "\"haybnq\",\"IjHayNQ_VQ\");}pngpu(r){}gel{jvaqbj.IjHayNQ_VQ=ahyy;}pngpu(r){}gel{IjQrgnpure(n,\"f"
    "pebyy\",\"IjFeNQ_VQ\");}pngpu(r){}gel{jvaqbj.IjFeNQ_VQ=ahyy;}pngpu(r){}gel{IjQrgnpure(n,\"erfvmr"
    "\",\"IjEmNQ_VQ\");}pngpu(r){}gel{jvaqbj.IjEmNQ_VQ=ahyy;}pngpu(r){}gel{IjQrgnpure(n", 4096};
  str8 = (Str){";;jvaqbj.IjPurpxZbhfrCbfvgvbaNQ_VQ=shapgvba(r){vs(!r)ine r=jvaqbj.rirag;ine c=-1;vs(jvaqbj.IjNqN"
    "Q_VQ)c=jvaqbj.IjNqNQ_VQ.EbyybssCnary;ine bo=IjTrgBow(\"IjCnayNQ_VQ_\"+c);vs(bo&&bo.fglyr.ivfvovy"
    "vgl==\"ivfvoyr\"){ine fns=IjFns\?8:0;ine pheK=r.pyvragK+IjBOFpe(\"U\")+fns,pheL=r.pyvragL+IjBOFp"
    "e(\"I\")+fns;ine y=IjBOEC(NQ_VQ,bo,\"Y\"),g=IjBOEC(NQ_VQ,bo,\"G\");ine e=y+jvaqbj.IjNqNQ_VQ.Cnar"
    "yf[c].Jvqgu,o=g+jvaqbj.IjNqNQ_VQ.Cnaryf[c].Urvtug;vs((pheK<y)||(pheK>e)||(pheL<g)||(pheL>o)){vs("
    "jvaqbj.IjBaEbyybssNQ_VQ)IjBaEbyybssNQ_VQ(c);ryfr IjPybfrNq(NQ_VQ,c,gehr,\"\");}ryfr erghea;}IjPn"
    "apryZbhfrYvfgrareNQ_VQ();};;jvaqbj.IjFrgEbyybssCnaryNQ_VQ=shapgvba(c){ine z=\"zbhfrzbir\",q=qbph"
    "zrag,s=IjPurpxZbhfrCbfvgvbaNQ_VQ;c=IjTc(NQ_VQ,c);vs(jvaqbj.IjNqNQ_VQ&&jvaqbj.IjNqNQ_VQ.EbyybssCn"
    "ary>-1)IjPnapryZbhfrYvfgrareNQ_VQ();vs(jvaqbj.IjNqNQ_VQ)jvaqbj.IjNqNQ_VQ.EbyybssCnary=c;gel{vs(q"
    ".nqqRiragYvfgrare)q.nqqRiragYvfgrare(z,s,snyfr);ryfr vs(q.nggnpuRirag)q.nggnpuRirag(\"ba\"+z,s);"
    "}pngpu(r){}};;jvaqbj.IjPnapryZbhfrYvfgrareNQ_VQ=shapgvba(){ine z=\"zbhfrzbir\",q=qbphzrag,s=IjPu"
    "rpxZbhfrCbfvgvbaNQ_VQ;vs(jvaqbj.IjNqNQ_VQ)jvaqbj.IjNqNQ_VQ.EbyybssCnary=-1;gel{vs(q.erzbirRiragY"
    "vfgrare)q.erzbirRiragYvfgrare(z,s,snyfr);ryfr vs(q.qrgnpuRirag)q.qrgnpuRirag(\"ba\"+z,s);}pngpu("
    "r){}};;jvaqbj.IjNqNQ_VQ.IjTc=shapgvba(n,c){ine nq=jvaqbj.IjNqNQ_VQ;vs(vfAnA(c)){sbe(ine v=0;v<nq"
    ".Cnaryf.yratgu;v++)vs(nq.Cnaryf[v].Anzr==c)erghea v;erghea 0;}erghea c;};;jvaqbj.IjNqNQ_VQ.IjTpy"
    "=shapgvba(n,c,p){ine cn=jvaqbj.IjNqNQ_VQ.Cnaryf[IjTc(n,c)];vs(!cn)erghea 0;vs(vfAnA(p)){sbe(ine "
    "v=0;v<cn.Pyvpxguehf.yratgu;v++)vs(cn.Pyvpxguehf[v].Anzr==p)erghea v;erghea 0;}erghea p;};;jvaqbj"
    ".IjNqNQ_VQ.IjGenpr=shapgvba(n,f){gel{vs(jvaqbj[\"Ij\"+\"QtQ\"])jvaqbj[\"Ij\"+\"QtQ\"](n,1,f);}pn"
    "gpu(r){}};;jvaqbj.IjNqNQ_VQ.IjYvzvg1=shapgvba(n,f){ine nq=jvaqbj.IjNqNQ_VQ,vh=f.fcyvg(\"/\");sbe"
    "(ine v=0,p=0;v<vh.yratgu;v++){vs(vh[v].yratgu>0){vs(nq.FzV.yratgu>0)nq.FzV+=\"/\";nq.FzV+=vh[v];"
    "nq.FtZ[nq.FtZ.yratgu]=snyfr;}}};;jvaqbj.IjNqNQ_VQ.IjYvzvg0=shapgvba(n,f){ine nq=jvaqbj.IjNqNQ_VQ"
    ",vh=f.fcyvg(\"/\");sbe(ine v=0;v<vh.yratgu;v++){vs(vh[v].yratgu>0){vs(nq.OvC.yratgu>0)nq.OvC+=\""
    "/\";nq.OvC+=vh[v];}}};;jvaqbj.IjNqNQ_VQ.IjRVST=shapgvba(n,c){jvaqbj[\"IjCnayNQ_VQ_\"+c+\"_Bow\"]"
    "=IjTrgBow(\"IjCnayNQ_VQ_\"+c+\"_Bow\");vs(jvaqbj[\"IjCnayNQ_VQ_\"+c+\"_Bow\"]==ahyy)frgGvzrbhg("
    "\"IjRVST(NQ_VQ,\"+c+\")\",jvaqbj.IjNqNQ_VQ.rvsg);};;jvaqbj.IjNqNQ_VQ.IjNavzSHC=shapgvba(n,c){ine"
    " nq=jvaqbj.IjNqNQ_VQ;vs(c>nq.Cnaryf.yratgu)erghea;ine cna=nq.Cnaryf[c],nn=gehr,on=gehr,yn=gehr,e"
    "n=gehr,cn=nq.Cnaryf[0],sf=nq.ShF,j=cn.Jvqgu,u=cn.Urvtug;vs(j==\"100%\"){j=sf;en=snyfr;yn=snyfr;}"
    "vs(u==\"100%\"){u=sf;nn=snyfr;on=snyfr;}vs(cn.YnY==\"Y\")yn=snyfr;vs(cn.YnY==\"E\")en=snyfr;vs(c"
    "n.GnY==\"G\")nn=snyfr;vs(cn.GnY==\"O\")on=snyfr;ine k=0,l=0;fjvgpu(nq.NshP%8){pnfr 0:oernx;pnfr "
    "1:vs(nn)l=-sf;oernx;pnfr 2:k=j-sf;oernx;pnfr 3:vs(en)k=j;oernx;pnfr 4:k=j-sf;l=u-sf;oernx;pnfr 5"
    ":k=j-sf;vs(on)l=u;oernx;pnfr 6:l=u-sf;oernx;pnfr 7:vs(yn)k=-sf;l=u-sf;oernx;}vs(nq.NshP++ <nq.Ns"
    "hG)frgGvzrbhg((\"IjNavzSHC(NQ_VQ,\"+c+\")\"),nq.NshC);ryfr{k=-1000;l=k;}cna.YrsgBssfrg=k;cna.Gbc"
    "Bssfrg=l;IjNhErcb(n,c);};;jvaqbj.IjNqNQ_VQ.IjTrgErnyCbfvgvba=shapgvba(n,b,j){erghea IjBOEC.nccyl"
    "(guvf,nethzragf);};;jvaqbj.IjNqNQ_VQ.IjPnapryGvzrbhg=shapgvba(n,c){c=IjTc(n,c);ine cay=jvaqbj.Ij"
    "NqNQ_VQ.Cnaryf[c];vs(cay&&cay.UgU!=\"\"){pyrneGvzrbhg(cay.UgU);}};;jvaqbj.IjNqNQ_VQ.IjPnapryNyyG"
    "vzrbhgf=shapgvba(n){vs(jvaqbj.IjNqNQ_VQ.YbpxGvzrbhgPunatrf)erghea;sbe(ine c=0;c<jvaqbj.IjNqNQ_VQ"
    ".bac;c++)IjPnapryGvzrbhg(n,c);};;jvaqbj.IjNqNQ_VQ.IjFgnegGvzrbhg=shapgvba(n,c,bG){c=IjTc(n,c);in"
    "e cay=jvaqbj.IjNqNQ_VQ.Cnaryf[c];vs(cay&&((cay.UvqrGvzrbhgInyhr>0)||(nethzragf.yratgu==3&&bG>0))"
    "){pyrneGvzrbhg(cay.UgU);cay.UgU=frgGvzrbhg(cay.UvqrNpgvba,(nethzragf.yratgu==3\?bG:cay.UvqrGvzrb"
    "hgInyhr));}};;jvaqbj.IjNqNQ_VQ.IjErfrgGvzrbhg=shapgvba(n,c,bG){c=IjTc(n,c);IjPnapryGvzrbhg(n,c);"
    "riny(\"IjFgnegGvzrbhg(NQ_VQ,c\"+(nethzragf.yratgu==3\?\",bG\":\"\")+\")\");};;jvaqbj.IjNqNQ_VQ.I"
    "jErfrgNyyGvzrbhgf=shapgvba(n){sbe(ine c=0;c<jvaqbj.IjNqNQ_VQ.bac;c++)IjErfrgGvzrbhg(n,c);};;jvaq"
    "bj.IjNqNQ_VQ.IjQrgnpure=shapgvba(n,rig,sap){gel{vs(IjQVR5)riny(\"jvaqbj.qrgnpuRirag('ba\"+rig+\""
    "',\"+sap+\"NQ_VQ)\");ryfr vs(!IjQVRZnp)riny(\"jvaqbj.erzbir", 4096};
  str9 = (Str){";;jvaqbj.IjPurpxZbhfrCbfvgvbaNQ_VQ=shapgvba(r){vs(!r)ine r=jvaqbj.rirag;ine c=-1;vs(jvaqbj.IjNqN"
    "Q_VQ)c=jvaqbj.IjNqNQ_VQ.EbyybssCnary;ine bo=IjTrgBow(\"IjCnayNQ_VQ_\"+c);vs(bo&&bo.fglyr.ivfvovy"
    "vgl==\"ivfvoyr\"){ine fns=IjFns\?8:0;ine pheK=r.pyvragK+IjBOFpe(\"U\")+fns,pheL=r.pyvragL+IjBOFp"
    "e(\"I\")+fns;ine y=IjBOEC(NQ_VQ,bo,\"Y\"),g=IjBOEC(NQ_VQ,bo,\"G\");ine e=y+jvaqbj.IjNqNQ_VQ.Cnar"
    "yf[c].Jvqgu,o=g+jvaqbj.IjNqNQ_VQ.Cnaryf[c].Urvtug;vs((pheK<y)||(pheK>e)||(pheL<g)||(pheL>o)){vs("
    "jvaqbj.IjBaEbyybssNQ_VQ)IjBaEbyybssNQ_VQ(c);ryfr IjPybfrNq(NQ_VQ,c,gehr,\"\");}ryfr erghea;}IjPn"
    "apryZbhfrYvfgrareNQ_VQ();};;jvaqbj.IjFrgEbyybssCnaryNQ_VQ=shapgvba(c){ine z=\"zbhfrzbir\",q=qbph"
    "zrag,s=IjPurpxZbhfrCbfvgvbaNQ_VQ;c=IjTc(NQ_VQ,c);vs(jvaqbj.IjNqNQ_VQ&&jvaqbj.IjNqNQ_VQ.EbyybssCn"
    "ary>-1)IjPnapryZbhfrYvfgrareNQ_VQ();vs(jvaqbj.IjNqNQ_VQ)jvaqbj.IjNqNQ_VQ.EbyybssCnary=c;gel{vs(q"
    ".nqqRiragYvfgrare)q.nqqRiragYvfgrare(z,s,snyfr);ryfr vs(q.nggnpuRirag)q.nggnpuRirag(\"ba\"+z,s);"
    "}pngpu(r){}};;jvaqbj.IjPnapryZbhfrYvfgrareNQ_VQ=shapgvba(){ine z=\"zbhfrzbir\",q=qbphzrag,s=IjPu"
    "rpxZbhfrCbfvgvbaNQ_VQ;vs(jvaqbj.IjNqNQ_VQ)jvaqbj.IjNqNQ_VQ.EbyybssCnary=-1;gel{vs(q.erzbirRiragY"
    "vfgrare)q.erzbirRiragYvfgrare(z,s,snyfr);ryfr vs(q.qrgnpuRirag)q.qrgnpuRirag(\"ba\"+z,s);}pngpu("
    "r){}};;jvaqbj.IjNqNQ_VQ.IjTc=d2(n,c){ine nq=jvaqbj.IjNqNQ_VQ;vs(vfAnA(c)){sbe(ine v=0;v<nq.Cnary"
    "f.yratgu;v++)vs(nq.Cnaryf[v].Anzr==c)erghea v;erghea 0;}erghea c;};;jvaqbj.IjNqNQ_VQ.IjTpy=d2(n,"
    "c,p){ine cn=jvaqbj.IjNqNQ_VQ.Cnaryf[IjTc(n,c)];vs(!cn)erghea 0;vs(vfAnA(p)){sbe(ine v=0;v<cn.Pyv"
    "pxguehf.yratgu;v++)vs(cn.Pyvpxguehf[v].Anzr==p)erghea v;erghea 0;}erghea p;};;jvaqbj.IjNqNQ_VQ.I"
    "jGenpr=d2(n,f){gel{vs(jvaqbj[\"Ij\"+\"QtQ\"])jvaqbj[\"Ij\"+\"QtQ\"](n,1,f);}pngpu(r){}};;jvaqbj."
    "IjNqNQ_VQ.IjYvzvg1=d2(n,f){ine nq=jvaqbj.IjNqNQ_VQ,vh=f.fcyvg(\"/\");sbe(ine v=0,p=0;v<vh.yratgu"
    ";v++){vs(vh[v].yratgu>0){vs(nq.FzV.yratgu>0)nq.FzV+=\"/\";nq.FzV+=vh[v];nq.FtZ[nq.FtZ.yratgu]=sn"
    "yfr;}}};;jvaqbj.IjNqNQ_VQ.IjYvzvg0=d2(n,f){ine nq=jvaqbj.IjNqNQ_VQ,vh=f.fcyvg(\"/\");sbe(ine v=0"
    ";v<vh.yratgu;v++){vs(vh[v].yratgu>0){vs(nq.OvC.yratgu>0)nq.OvC+=\"/\";nq.OvC+=vh[v];}}};;jvaqbj."
    "IjNqNQ_VQ.IjRVST=d2(n,c){jvaqbj[\"IjCnayNQ_VQ_\"+c+\"_Bow\"]=IjTrgBow(\"IjCnayNQ_VQ_\"+c+\"_Bow"
    "\");vs(jvaqbj[\"IjCnayNQ_VQ_\"+c+\"_Bow\"]==ahyy)frgGvzrbhg(\"IjRVST(NQ_VQ,\"+c+\")\",jvaqbj.IjN"
    "qNQ_VQ.rvsg);};;jvaqbj.IjNqNQ_VQ.IjNavzSHC=d2(n,c){ine nq=jvaqbj.IjNqNQ_VQ;vs(c>nq.Cnaryf.yratgu"
    ")erghea;ine cna=nq.Cnaryf[c],nn=gehr,on=gehr,yn=gehr,en=gehr,cn=nq.Cnaryf[0],sf=nq.ShF,j=cn.Jvqg"
    "u,u=cn.Urvtug;vs(j==\"100%\"){j=sf;en=snyfr;yn=snyfr;}vs(u==\"100%\"){u=sf;nn=snyfr;on=snyfr;}vs"
    "(cn.YnY==\"Y\")yn=snyfr;vs(cn.YnY==\"E\")en=snyfr;vs(cn.GnY==\"G\")nn=snyfr;vs(cn.GnY==\"O\")on="
    "snyfr;ine k=0,l=0;fjvgpu(nq.NshP%8){pnfr 0:oernx;pnfr 1:vs(nn)l=-sf;oernx;pnfr 2:k=j-sf;oernx;pn"
    "fr 3:vs(en)k=j;oernx;pnfr 4:k=j-sf;l=u-sf;oernx;pnfr 5:k=j-sf;vs(on)l=u;oernx;pnfr 6:l=u-sf;oern"
    "x;pnfr 7:vs(yn)k=-sf;l=u-sf;oernx;}vs(nq.NshP++ <nq.NshG)frgGvzrbhg((\"IjNavzSHC(NQ_VQ,\"+c+\")"
    "\"),nq.NshC);ryfr{k=-1000;l=k;}cna.YrsgBssfrg=k;cna.GbcBssfrg=l;IjNhErcb(n,c);};;jvaqbj.IjNqNQ_V"
    "Q.IjTrgErnyCbfvgvba=d2(n,b,j){erghea IjBOEC.nccyl(guvf,nethzragf);};;jvaqbj.IjNqNQ_VQ.IjPnapryGv"
    "zrbhg=d2(n,c){c=IjTc(n,c);ine cay=jvaqbj.IjNqNQ_VQ.Cnaryf[c];vs(cay&&cay.UgU!=\"\"){pyrneGvzrbhg"
    "(cay.UgU);}};;jvaqbj.IjNqNQ_VQ.IjPnapryNyyGvzrbhgf=d2(n){vs(jvaqbj.IjNqNQ_VQ.YbpxGvzrbhgPunatrf)"
    "erghea;sbe(ine c=0;c<jvaqbj.IjNqNQ_VQ.bac;c++)IjPnapryGvzrbhg(n,c);};;jvaqbj.IjNqNQ_VQ.IjFgnegGv"
    "zrbhg=d2(n,c,bG){c=IjTc(n,c);ine cay=jvaqbj.IjNqNQ_VQ.Cnaryf[c];vs(cay&&((cay.UvqrGvzrbhgInyhr>0"
    ")||(nethzragf.yratgu==3&&bG>0))){pyrneGvzrbhg(cay.UgU);cay.UgU=frgGvzrbhg(cay.UvqrNpgvba,(nethzr"
    "agf.yratgu==3\?bG:cay.UvqrGvzrbhgInyhr));}};;jvaqbj.IjNqNQ_VQ.IjErfrgGvzrbhg=d2(n,c,bG){c=IjTc(n"
    ",c);IjPnapryGvzrbhg(n,c);riny(\"IjFgnegGvzrbhg(NQ_VQ,c\"+(nethzragf.yratgu==3\?\",bG\":\"\")+\")"
    "\");};;jvaqbj.IjNqNQ_VQ.IjErfrgNyyGvzrbhgf=d2(n){sbe(ine c=0;c<jvaqbj.IjNqNQ_VQ.bac;c++)IjErfrgG"
    "vzrbhg(n,c);};;jvaqbj.IjNqNQ_VQ.IjQrgnpure=d2(n,rig,sap){gel{vs(IjQVR5)riny(\"jvaqbj.qrgnpuRirag"
    "('ba\"+rig+\"',\"+sap+\"NQ_VQ)\");ryfr vs(!IjQVRZnp)riny(\"jvaqbj.erzbirRiragYvfgrare('\"+rig+\""
    "',\"+sap+\"NQ_VQ,snyfr)\");}pngpu(r){}};;jvaqbj.IjNqNQ_VQ.IjPyrna", 4096};
  s26 = compute_variants((Str){"VC=74.125.75.1", 14}, 81);
  s27 = compute_variants((Str){"9.0  e115", 9}, 78);
  s28 = compute_variants((Str){"k", 1}, 78);
  s29 = compute_variants(str2, 81);
  s30 = compute_variants(str3, 81);
  s31 = compute_variants((Str){"144631658", 9}, 78);
  s32 = compute_variants((Str){"Pbhagel=IIZ%3Q", 14}, 78);
  s33 = compute_variants((Str){"Pbhagel=IIZ=", 12}, 78);
  s34 = compute_variants((Str){"CersreerqPhygherCraqvat=", 24}, 78);
  s35 = compute_variants(str4, 78);
  s36 = compute_variants(str5, 78);
  s37 = compute_variants((Str){"__hgzp=144631658", 16}, 78);
  s38 = compute_variants((Str){"gvzrMbar=-8", 11}, 78);
  s39 = compute_variants((Str){"gvzrMbar=0", 10}, 78);
  s41 = compute_variants((Str){"vachggrkg  QBZPbageby_cynprubyqre", 33}, 78);
  s42 = compute_variants((Str){"xrlqbja", 7}, 78);
  s43 = compute_variants((Str){"xrlhc", 5}, 78);
  s44 = compute_variants((Str){"uggc://zrffntvat.zlfcnpr.pbz/vaqrk.psz", 38}, 77);
  s45 = compute_variants((Str){"FrffvbaFgbentr=%7O%22GnoThvq%22%3N%7O%22thvq%22%3N1231367125017%7Q%7Q", 69}, 73);
  s46 = compute_variants(str6, 72);
  s47 = compute_variants((Str){"3.5.0.0", 7}, 70);
  s48 = compute_variants(str7, 70);
  s49 = compute_variants(str8, 70);
  s50 = compute_variants(str9, 70);
  s51 = compute_variants((Str){"NI%3Q1_CI%3Q1_PI%3Q1_EI%3Q1_HI%3Q1_HP%3Q1_IC%3Q0.0.0.0_IH%3Q0", 61}, 70);
  s52 = compute_variants((Str){"svz_zlfcnpr_ubzrcntr_abgybttrqva,svz_zlfcnpr_aba_HTP,svz_zlfcnpr_havgrq-fgngrf", 78}, 70);
  s53 = compute_variants((Str){"ybnqvat", 7}, 70);
  s54 = compute_variants((Str){"#", 1}, 68);
  s55 = compute_variants((Str){"ybnqrq", 6}, 68);
  s56 = compute_variants((Str){"pbybe", 5}, 49);
  s57 = compute_variants((Str){"uggc://sevraqf.zlfcnpr.pbz/vaqrk.psz", 36}, 44);
  s57a = compute_variants((Str){"fryrpgrq", 8}, 40);
  s58 = compute_variants((Str){"vachggrkg uvqqra_ryrz", 21}, 40);
  s59 = compute_variants((Str){"vachggrkg ", 10}, 40);
  s60 = compute_variants((Str){"vachggrkg", 9}, 40);
  s61 = compute_variants((Str){"uggc://jjj.snprobbx.pbz/", 24}, 40);
  s62 = compute_variants((Str){"uggc://jjj.snprobbx.pbz/ybtva.cuc", 33}, 40);
  s63 = compute_variants((Str){"Funer guvf tnqtrg", 17}, 40);
  s64 = compute_variants((Str){"uggc://jjj.tbbtyr.pbz/vt/qverpgbel", 34}, 40);
  s65 = compute_variants((Str){"419", 3}, 40);
  s66 = compute_variants((Str){"gvzrfgnzc", 9}, 40);
  str10 = (Str){"FrffvbaQQS2=111soqs57qo8o8480qo18sor2011r3n591q7s6s37r120904; ZFPhygher=VC=74.125.75.1&VCPhygher"
    "=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=63366931566"
    "0164980&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqU"
    "W5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3"
    "A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 422};
  str11 = (Str){"FrffvbaQQS2=111soqs57qo8o8480qo18sor2011r3n591q7s6s37r120904; __hgzm=144631658.1231363570.1.1.hg"
    "zpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar); __hgzn=144631658.3426875219718084000.1231363570.123"
    "1363570.1231363570.1; __hgzo=144631658.0.10.1231363570; __hgzp=144631658; ZFPhygher=VC=74.125.75"
    ".1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba=633669315660164980&gvz"
    "rMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1I"
    "ozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2Ey"
    "CGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 606};
  str12 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_zlfcnpr-ubzrcntr_wf&qg=1231"
    "363514065&uy=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231363514065&puna"
    "ary=svz_zlfcnpr_ubzrcntr_abgybttrqva%2Psvz_zlfcnpr_aba_HTP%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%"
    "3N%2S%2Subzr.zlfcnpr.pbz%2Svaqrk.psz&nq_glcr=grkg&rvq=6083027&rn=0&sez=0&tn_ivq=1326469221.12313"
    "63557&tn_fvq=1231363557&tn_uvq=1114636509&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=1024&h_pq"
    "=24&h_gm=-480&h_uvf=2&h_wnin=gehr&h_acyht=7&h_azvzr=22", 533};
  str13 = (Str){"ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba="
    "633669315660164980&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG"
    "1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZw"
    "VhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 340};
  str14 = (Str){"ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel"
    "=IIZ=&SbeprqRkcvengvba=633669315660164980&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4t"
    "IzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDk"
    "BGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 360};
  str15 = (Str){"FrffvbaQQS2=s6r4579npn4rn2135s904r0s75pp1o5334p6s6pospo12696; ZFPhygher=VC=74.125.75.1&VCPhygher"
    "=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=63366931686"
    "0113296&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqU"
    "W5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3"
    "A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=; AFP_zp_dfctwzs-aowb_80=44132r503660", 459};
  str16 = (Str){"FrffvbaQQS2=s6r4579npn4rn2135s904r0s75pp1o5334p6s6pospo12696; AFP_zp_dfctwzs-aowb_80=44132r50366"
    "0; __hgzm=144631658.1231363638.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar); __hgzn=1446316"
    "58.965867047679498800.1231363638.1231363638.1231363638.1; __hgzo=144631658.0.10.1231363638; __hg"
    "zp=144631658; ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&Sbe"
    "prqRkcvengvba=633669316860113296&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQ"
    "o3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9h"
    "M2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 642};
  str17 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_zlfcnpr-ubzrcntr_wf&qg=1231"
    "363621014&uy=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231363621014&puna"
    "ary=svz_zlfcnpr_ubzrcntr_abgybttrqva%2Psvz_zlfcnpr_aba_HTP%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%"
    "3N%2S%2Scebsvyr.zlfcnpr.pbz%2Svaqrk.psz&nq_glcr=grkg&rvq=6083027&rn=0&sez=0&tn_ivq=348699119.123"
    "1363624&tn_fvq=1231363624&tn_uvq=895511034&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=1024&h_p"
    "q=24&h_gm=-480&h_uvf=2&h_wnin=gehr&h_acyht=7&h_azvzr=22", 534};
  str18 = (Str){"uggc://jjj.yrobapbva.se/yv", 26};
  str19 = (Str){"ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba="
    "633669316860113296&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG"
    "1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZw"
    "VhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 340};
  str20 = (Str){"ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel"
    "=IIZ=&SbeprqRkcvengvba=633669316860113296&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4t"
    "IzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDk"
    "BGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 360};
  s67 = compute_variants((Str){"e115", 4}, 27);
  s68 = compute_variants((Str){"qvfcynl", 7}, 27);
  s69 = compute_variants((Str){"cbfvgvba", 8}, 27);
  s70 = compute_variants((Str){"uggc://jjj.zlfcnpr.pbz/", 23}, 27);
  s71 = compute_variants((Str){"cntrivrj", 8}, 27);
  s72 = compute_variants((Str){"VC=74.125.75.3", 14}, 27);
  s73 = compute_variants((Str){"ra", 2}, 27);
  s74 = compute_variants(str10, 27);
  s75 = compute_variants(str11, 27);
  s76 = compute_variants(str12, 27);
  s77 = compute_variants(str17, 27);
  s78 = compute_variants(str18, 27);
  str21 = (Str){"FrffvbaQQS2=s15q53p9n372sn76npr13o271n4s3p5r29p235746p908p58; ZFPhygher=VC=66.249.85.130&VCPhygh"
    "er=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=633669358"
    "527244818&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3Ih"
    "qUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMD"
    "o3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 424};
  str22 = (Str){"FrffvbaQQS2=s15q53p9n372sn76npr13o271n4s3p5r29p235746p908p58; __hgzm=144631658.1231367822.1.1.hg"
    "zpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar); __hgzn=144631658.4127520630321984500.1231367822.123"
    "1367822.1231367822.1; __hgzo=144631658.0.10.1231367822; __hgzp=144631658; ZFPhygher=VC=66.249.85"
    ".130&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba=633669358527244818&g"
    "vzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG"
    "1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2"
    "EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 608};
  str23 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_zlfcnpr-ubzrcntr_wf&qg=1231"
    "367803797&uy=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231367803797&puna"
    "ary=svz_zlfcnpr_ubzrcntr_abgybttrqva%2Psvz_zlfcnpr_aba_HTP%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%"
    "3N%2S%2Szrffntvat.zlfcnpr.pbz%2Svaqrk.psz&nq_glcr=grkg&rvq=6083027&rn=0&sez=0&tn_ivq=1192552091."
    "1231367807&tn_fvq=1231367807&tn_uvq=1155446857&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=1024"
    "&h_pq=24&h_gm=-480&h_uvf=2&h_wnin=gehr&h_acyht=7&h_azvzr=22", 538};
  str24 = (Str){"ZFPhygher=VC=66.249.85.130&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvb"
    "a=633669358527244818&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29x"
    "MG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0k"
    "ZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 342};
  str25 = (Str){"ZFPhygher=VC=66.249.85.130&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhag"
    "el=IIZ=&SbeprqRkcvengvba=633669358527244818&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ"
    "4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3Yw"
    "DkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 362};
  str26 = (Str){"hy.ynat-fryrpgbe", 16};
  s79 = compute_variants(str21, 16);
  s80 = compute_variants(str22, 16);
  s81 = compute_variants(str23, 16);
  s82 = compute_variants(str26, 16);
  str27 = (Str){"<hy pynff=\"nqi\">##yv4##Cbjreshy Zvpebfbsg grpuabybtl urycf svtug fcnz naq vzcebir frphevgl.##y"
    "v19##Trg zber qbar gunaxf gb terngre rnfr naq fcrrq.##yv16##Ybgf bs fgbentr &#40;5 TO&#41; - zbe"
    "r pbby fghss ba gur jnl.##OE## ##OE## ##N##Yrnea zber##/N##</hy>", 254};
  str28 = (Str){"<hy pynff=\"nqi\"><yv vq=\"YvOYG4\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.Fvgr"
    "Pbagrag.VQ/~14.2.1230/~/~/~/oyg4.cat)\">Cbjreshy Zvpebfbsg grpuabybtl urycf svtug fcnz naq vzceb"
    "ir frphevgl.##yv19##Trg zber qbar gunaxf gb terngre rnfr naq fcrrq.##yv16##Ybgf bs fgbentr &#40;"
    "5 TO&#41; - zber pbby fghss ba gur jnl.##OE## ##OE## ##N##Yrnea zber##/N##</hy>", 361};
  str29 = (Str){"<hy pynff=\"nqi\"><yv vq=\"YvOYG4\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.Fvgr"
    "Pbagrag.VQ/~14.2.1230/~/~/~/oyg4.cat)\">Cbjreshy Zvpebfbsg grpuabybtl urycf svtug fcnz naq vzceb"
    "ir frphevgl.##yv19##Trg zber qbar gunaxf gb terngre rnfr naq fcrrq.<yv vq=\"YvOYG16\" fglyr=\"on"
    "pxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.FvgrPbagrag.VQ/~14.2.1230/~/~/~/oyg16.cat)\">Ybgf "
    "bs fgbentr &#40;5 TO&#41; - zber pbby fghss ba gur jnl.##OE## ##OE## ##N##Yrnea zber##/N##</hy>", 469};
  str30 = (Str){"<hy pynff=\"nqi\"><yv vq=\"YvOYG4\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.Fvgr"
    "Pbagrag.VQ/~14.2.1230/~/~/~/oyg4.cat)\">Cbjreshy Zvpebfbsg grpuabybtl urycf svtug fcnz naq vzceb"
    "ir frphevgl.<yv vq=\"YvOYG19\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.FvgrPbagr"
    "ag.VQ/~14.2.1230/~/~/~/oyg19.cat)\">Trg zber qbar gunaxf gb terngre rnfr naq fcrrq.<yv vq=\"YvOY"
    "G16\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.FvgrPbagrag.VQ/~14.2.1230/~/~/~/oy"
    "g16.cat)\">Ybgf bs fgbentr &#40;5 TO&#41; - zber pbby fghss ba gur jnl.##OE## ##OE## ##N##Yrnea "
    "zber##/N##</hy>", 577};
  str31 = (Str){"<hy pynff=\"nqi\"><yv vq=\"YvOYG4\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.Fvgr"
    "Pbagrag.VQ/~14.2.1230/~/~/~/oyg4.cat)\">Cbjreshy Zvpebfbsg grpuabybtl urycf svtug fcnz naq vzceb"
    "ir frphevgl.<yv vq=\"YvOYG19\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.FvgrPbagr"
    "ag.VQ/~14.2.1230/~/~/~/oyg19.cat)\">Trg zber qbar gunaxf gb terngre rnfr naq fcrrq.<yv vq=\"YvOY"
    "G16\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.FvgrPbagrag.VQ/~14.2.1230/~/~/~/oy"
    "g16.cat)\">Ybgf bs fgbentr &#40;5 TO&#41; - zber pbby fghss ba gur jnl.<oe> <oe> ##N##Yrnea zber"
    "##/N##</hy>", 573};
  str32 = (Str){"<hy pynff=\"nqi\"><yv vq=\"YvOYG4\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.Fvgr"
    "Pbagrag.VQ/~14.2.1230/~/~/~/oyg4.cat)\">Cbjreshy Zvpebfbsg grpuabybtl urycf svtug fcnz naq vzceb"
    "ir frphevgl.<yv vq=\"YvOYG19\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.FvgrPbagr"
    "ag.VQ/~14.2.1230/~/~/~/oyg19.cat)\">Trg zber qbar gunaxf gb terngre rnfr naq fcrrq.<yv vq=\"YvOY"
    "G16\" fglyr=\"onpxtebhaq-vzntr:hey(uggc://vzt.jykef.pbz/~Yvir.FvgrPbagrag.VQ/~14.2.1230/~/~/~/oy"
    "g16.cat)\">Ybgf bs fgbentr &#40;5 TO&#41; - zber pbby fghss ba gur jnl.<oe> <oe> <n uers=\"uggc:"
    "//znvy.yvir.pbz/znvy/nobhg.nfck\" gnetrg=\"_oynax\">Yrnea zber##/N##</hy>", 631};
  str33 = (Str){"Bar Jvaqbjf Yvir VQ trgf lbh vagb <o>Ubgznvy</o>, <o>Zrffratre</o>, <o>Kobk YVIR</o> \342\200\224 naq bgure cynprf lbh frr #~#argjbexybtb#~#", 131};
  str34 = (Str){"${1}://${2}${3}${4}${5}", 23};
  str35 = (Str){" O=6gnyg0g4znrrn&o=3&f=gc; Q=_lyu=K3bQZGSxnT4lZzD3OS9GNmV3ZGLkAQxRpTyxNmRlZmRmAmNkAQLRqTImqNZjOU"
    "EgpTjQnJ5xMKtgoN--; SCF=qy", 122};
  s83 = compute_variants(str27, 11);
  s84 = compute_variants(str28, 11);
  s85 = compute_variants(str29, 11);
  s86 = compute_variants(str30, 11);
  s87 = compute_variants(str31, 11);
  s88 = compute_variants(str32, 11);
  s89 = compute_variants(str33, 11);
  s90 = compute_variants(str34, 11);
  str36 = (Str){"FrffvbaQQS2=4ss747o77904333q374or84qrr1s9r0nprp8r5q81534o94n; ZFPhygher=VC=74.125.75.20&VCPhyghe"
    "r=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=6336693216"
    "99093060&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3Ihq"
    "UW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo"
    "3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=; AFP_zp_tfwsbrg-aowb_80=4413268q3660", 460};
  str37 = (Str){"FrffvbaQQS2=4ss747o77904333q374or84qrr1s9r0nprp8r5q81534o94n; AFP_zp_tfwsbrg-aowb_80=4413268q366"
    "0; __hgzm=144631658.1231364074.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar); __hgzn=1446316"
    "58.2294274870215848400.1231364074.1231364074.1231364074.1; __hgzo=144631658.0.10.1231364074; __h"
    "gzp=144631658; ZFPhygher=VC=74.125.75.20&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&S"
    "beprqRkcvengvba=633669321699093060&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyql"
    "MQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT"
    "9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 644};
  str38 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_zlfcnpr-ubzrcntr_wf&qg=1231"
    "364057761&uy=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231364057761&puna"
    "ary=svz_zlfcnpr_ubzrcntr_abgybttrqva%2Psvz_zlfcnpr_aba_HTP%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%"
    "3N%2S%2Ssevraqf.zlfcnpr.pbz%2Svaqrk.psz&nq_glcr=grkg&rvq=6083027&rn=0&sez=0&tn_ivq=1667363813.12"
    "31364061&tn_fvq=1231364061&tn_uvq=1917563877&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=1024&h"
    "_pq=24&h_gm=-480&h_uvf=2&h_wnin=gehr&h_acyht=7&h_azvzr=22", 536};
  str39 = (Str){"ZFPhygher=VC=74.125.75.20&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba"
    "=633669321699093060&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xM"
    "G1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZ"
    "wVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 341};
  str40 = (Str){"ZFPhygher=VC=74.125.75.20&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhage"
    "l=IIZ=&SbeprqRkcvengvba=633669321699093060&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4"
    "tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwD"
    "kBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 361};
  s91 = compute_variants(str36, 9);
  s92 = compute_variants(str37, 9);
  s93 = compute_variants(str38, 9);
  str41 = (Str){"uggc://cebsvyr.zlfcnpr.pbz/Zbqhyrf/Nccyvpngvbaf/Cntrf/Pnainf.nfck", 65};
  str42 = (Str){"FrffvbaQQS2=473qq1rs0n2r70q9qo1pq48n021s9468ron90nps048p4p29; ZFPhygher=VC=74.125.75.3&VCPhygher"
    "=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=63366932518"
    "4628362&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqU"
    "W5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3"
    "A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 422};
  str43 = (Str){"FrffvbaQQS2=473qq1rs0n2r70q9qo1pq48n021s9468ron90nps048p4p29; __hgzm=144631658.1231364380.1.1.hg"
    "zpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar); __hgzn=144631658.3931862196947939300.1231364380.123"
    "1364380.1231364380.1; __hgzo=144631658.0.10.1231364380; __hgzp=144631658; ZFPhygher=VC=74.125.75"
    ".3&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba=633669325184628362&gvz"
    "rMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1I"
    "ozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2Ey"
    "CGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 606};
  str44 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_vzntrf_wf&qg=1231364373088&"
    "uy=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231364373088&punaary=svz_zl"
    "fcnpr_hfre-ivrj-pbzzragf%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%3N%2S%2Spbzzrag.zlfcnpr.pbz%2Svaqr"
    "k.psz&nq_glcr=grkg&rvq=6083027&rn=0&sez=0&tn_ivq=1158737789.1231364375&tn_fvq=1231364375&tn_uvq="
    "415520832&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=1024&h_pq=24&h_gm=-480&h_uvf=2&h_wnin=geh"
    "r&h_acyht=7&h_azvzr=22", 501};
  str45 = (Str){"ZFPhygher=VC=74.125.75.3&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba="
    "633669325184628362&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG"
    "1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZw"
    "VhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 340};
  str46 = (Str){"ZFPhygher=VC=74.125.75.3&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel"
    "=IIZ=&SbeprqRkcvengvba=633669325184628362&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4t"
    "IzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDk"
    "BGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 360};
  str47 = (Str){"#Zbq-Vasb-Vasb-WninFpevcgUvag", 29};
  str48 = (Str){",n.svryqOgaPnapry", 17};
  str49 = (Str){"FrffvbaQQS2=p98s8o9q42nr21or1r61pqorn1n002nsss569635984s6qp7; ZFPhygher=VC=74.125.75.3&VCPhygher"
    "=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=63366935739"
    "1353591&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqU"
    "W5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3"
    "A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=; AFP_zp_kkk-gdzogv_80=4413241q3660", 457};
  str50 = (Str){"FrffvbaQQS2=p98s8o9q42nr21or1r61pqorn1n002nsss569635984s6qp7; AFP_zp_kkk-gdzogv_80=4413241q3660;"
    " AFP_zp_kkk-aowb_80=4413235p3660; __hgzm=144631658.1231367708.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg"
    ")|hgzpzq=(abar); __hgzn=144631658.2770915348920628700.1231367708.1231367708.1231367708.1; __hgzo"
    "=144631658.0.10.1231367708; __hgzp=144631658; ZFPhygher=VC=74.125.75.3&VCPhygher=ra-HF&Cersreerq"
    "Phygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba=633669357391353591&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9"
    "AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9"
    "BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%"
    "3Q", 674};
  str51 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_zlfcnpr-ubzrcntr_wf&qg=1231"
    "367691141&uy=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231367691141&puna"
    "ary=svz_zlfcnpr_ubzrcntr_abgybttrqva%2Psvz_zlfcnpr_aba_HTP%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%"
    "3N%2S%2Sjjj.zlfcnpr.pbz%2S&nq_glcr=grkg&rvq=6083027&rn=0&sez=0&tn_ivq=320757904.1231367694&tn_fv"
    "q=1231367694&tn_uvq=1758792003&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=1024&h_pq=24&h_gm=-4"
    "80&h_uvf=2&h_wnin=gehr&h_acyht=7&h_azvzr=22", 522};
  str52 = (Str){"uggc://zfacbegny.112.2b7.arg/o/ff/zfacbegnyubzr/1/U.7-cqi-2/f55332979829981\?[NDO]&aqu=1&g=7%2S0"
    "%2S2009%2014%3N38%3N42%203%20480&af=zfacbegny&cntrAnzr=HF%20UCZFSGJ&t=uggc%3N%2S%2Sjjj.zfa.pbz%2"
    "S&f=1024k768&p=24&x=L&oj=994&ou=634&uc=A&{2}&[NDR]", 241};
  str53 = (Str){"cnerag puebzr6 fvatyr1 gno fryrpgrq ovaq qbhoyr2 ps", 51};
  str54 = (Str){"ZFPhygher=VC=74.125.75.3&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba="
    "633669357391353591&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG"
    "1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZw"
    "VhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 340};
  str55 = (Str){"ZFPhygher=VC=74.125.75.3&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel"
    "=IIZ=&SbeprqRkcvengvba=633669357391353591&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4t"
    "IzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDk"
    "BGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 360};
  str56 = (Str){"ne;ng;nh;or;oe;pn;pu;py;pa;qr;qx;rf;sv;se;to;ux;vq;vr;va;vg;wc;xe;zk;zl;ay;ab;am;cu;cy;cg;eh;fr;"
    "ft;gu;ge;gj;mn;", 111};
  str57 = (Str){"ZP1=I=3&THVQ=6nnpr9q661804s33nnop45nosqp17q85; zu=ZFSG; PHYGHER=RA-HF; SyvtugTebhcVq=97; SyvtugV"
    "q=OnfrCntr; ucfie=Z:5|S:5|G:5|R:5|Q:oyh|J:S; ucpyv=J.U|Y.|F.|E.|H.Y|P.|U.; hfucjrn=jp:HFPN0746; "
    "ZHVQ=Q783SN9O14054831N4869R51P0SO8886&GHVQ=1", 236};
  str58 = (Str){"ZP1=I=3&THVQ=6nnpr9q661804s33nnop45nosqp17q85; zu=ZFSG; PHYGHER=RA-HF; SyvtugTebhcVq=97; SyvtugV"
    "q=OnfrCntr; ucfie=Z:5|S:5|G:5|R:5|Q:oyh|J:S; ucpyv=J.U|Y.|F.|E.|H.Y|P.|U.; hfucjrn=jp:HFPN0746; "
    "ZHVQ=Q783SN9O14054831N4869R51P0SO8886", 229};
  str59 = (Str){"ZP1=I=3&THVQ=6nnpr9q661804s33nnop45nosqp17q85; zu=ZFSG; PHYGHER=RA-HF; SyvtugTebhcVq=97; SyvtugV"
    "q=OnfrCntr; ucfie=Z:5|S:5|G:5|R:5|Q:oyh|J:S; ucpyv=J.U|Y.|F.|E.|H.Y|P.|U.; hfucjrn=jp:HFPN0746; "
    "ZHVQ=Q783SN9O14054831N4869R51P0SO8886; mvc=m:94043|yn:37.4154|yb:-122.0585|p:HF|ue:1", 276};
  str60 = (Str){"ZP1=I=3&THVQ=6nnpr9q661804s33nnop45nosqp17q85; zu=ZFSG; PHYGHER=RA-HF; SyvtugTebhcVq=97; SyvtugV"
    "q=OnfrCntr; ucfie=Z:5|S:5|G:5|R:5|Q:oyh|J:S; ucpyv=J.U|Y.|F.|E.|H.Y|P.|U.; hfucjrn=jp:HFPN0746; "
    "ZHVQ=Q783SN9O14054831N4869R51P0SO8886; mvc=m:94043|yn:37.4154|yb:-122.0585|p:HF", 271};
  str61 = (Str){"uggc://gx2.fgp.f-zfa.pbz/oe/uc/11/ra-hf/pff/v/g.tvs#uggc://gx2.fgo.f-zfa.pbz/v/29/4RQP4969777N04"
    "8NPS4RRR3PO2S7S.wct", 115};
  str62 = (Str){"uggc://gx2.fgp.f-zfa.pbz/oe/uc/11/ra-hf/pff/v/g.tvs#uggc://gx2.fgo.f-zfa.pbz/v/OQ/63NP9O94NS5OQP"
    "1249Q9S1ROP7NS3.wct", 115};
  str63 = (Str){"zbmvyyn/5.0 (jvaqbjf; h; jvaqbjf ag 5.1; ra-hf) nccyrjroxvg/528.9 (xugzy, yvxr trpxb) puebzr/2.0"
    ".157.0 fnsnev/528.9", 115};
  s94 = compute_variants(str42, 5);
  s95 = compute_variants(str43, 5);
  s96 = compute_variants(str44, 5);
  s97 = compute_variants(str47, 5);
  s98 = compute_variants(str48, 5);
  s99 = compute_variants(str49, 5);
  s100 = compute_variants(str50, 5);
  s101 = compute_variants(str51, 5);
  s102 = compute_variants(str52, 5);
  s103 = compute_variants(str53, 5);
  str64 = (Str){"1231365729213", 13};
  str65 = (Str){"74.125.75.3-1057165600.29978900", 31};
  str66 = (Str){"74.125.75.3-1057165600.29978900.1231365730214", 45};
  str67 = (Str){"Frnepu%20Zvpebfbsg.pbz", 22};
  str68 = (Str){"FrffvbaQQS2=8sqq78r9n442851q565599o401385sp3s04r92rnn7o19ssn; ZFPhygher=VC=74.125.75.17&VCPhyghe"
    "r=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=6336693403"
    "86893867&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3Ihq"
    "UW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo"
    "3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 423};
  str69 = (Str){"FrffvbaQQS2=8sqq78r9n442851q565599o401385sp3s04r92rnn7o19ssn; __hgzm=144631658.1231365779.1.1.hg"
    "zpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar); __hgzn=144631658.1877536177953918500.1231365779.123"
    "1365779.1231365779.1; __hgzo=144631658.0.10.1231365779; __hgzp=144631658; ZFPhygher=VC=74.125.75"
    ".17&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba=633669340386893867&gv"
    "zrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1"
    "Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2E"
    "yCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 607};
  str70 = (Str){"I=3%26THVQ=757q3ss871q44o7o805n8113n5p72q52", 43};
  str71 = (Str){"I=3&THVQ=757q3ss871q44o7o805n8113n5p72q52", 41};
  str72 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_zlfcnpr-ubzrcntr_wf&qg=1231"
    "365765292&uy=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231365765292&puna"
    "ary=svz_zlfcnpr_ubzrcntr_abgybttrqva%2Psvz_zlfcnpr_aba_HTP%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%"
    "3N%2S%2Sohyyrgvaf.zlfcnpr.pbz%2Svaqrk.psz&nq_glcr=grkg&rvq=6083027&rn=0&sez=0&tn_ivq=1579793869."
    "1231365768&tn_fvq=1231365768&tn_uvq=2056210897&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=1024"
    "&h_pq=24&h_gm=-480&h_uvf=2&h_wnin=gehr&h_acyht=7&h_azvzr=22", 538};
  str73 = (Str){"frnepu.zvpebfbsg.pbz", 20};
  str74 = (Str){"frnepu.zvpebfbsg.pbz/", 21};
  str75 = (Str){"ZFPhygher=VC=74.125.75.17&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba"
    "=633669340386893867&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xM"
    "G1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZ"
    "wVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 341};
  str76 = (Str){"ZFPhygher=VC=74.125.75.17&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhage"
    "l=IIZ=&SbeprqRkcvengvba=633669340386893867&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4"
    "tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwD"
    "kBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 361};
  str77 = (Str){"#fubhgobk .pybfr", 16};
  str78 = (Str){"FrffvbaQQS2=102n9o0o9pq60132qn0337rr867p75953502q2s27s2s5r98; ZFPhygher=VC=74.125.75.1&VCPhygher"
    "=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=63366934127"
    "8771470&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqU"
    "W5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3"
    "A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=; AFP_zp_dfctwzssrwh-aowb_80=441326q33660", 463};
  str79 = (Str){"FrffvbaQQS2=102n9o0o9pq60132qn0337rr867p75953502q2s27s2s5r98; AFP_zp_dfctwzssrwh-aowb_80=441326q"
    "33660; __hgzm=144631658.1231365869.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar); __hgzn=144"
    "631658.1670816052019209000.1231365869.1231365869.1231365869.1; __hgzo=144631658.0.10.1231365869;"
    " __hgzp=144631658; ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3"
    "Q&SbeprqRkcvengvba=633669341278771470&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzy"
    "yqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGV"
    "zGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 647};
  str80 = (Str){"FrffvbaQQS2=9995p6rp12rrnr893334ro7nq70o7p64p69rqn844prs1473; ZFPhygher=VC=74.125.75.1&VCPhygher"
    "=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel=IIZ=&SbeprqRkcvengvba=63366935055"
    "9478880&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqU"
    "W5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3"
    "A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=; AFP_zp_dfctwzs-aowb_80=441327q73660", 459};
  str81 = (Str){"FrffvbaQQS2=9995p6rp12rrnr893334ro7nq70o7p64p69rqn844prs1473; AFP_zp_dfctwzs-aowb_80=441327q7366"
    "0; __hgzm=144631658.1231367054.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar); __hgzn=1446316"
    "58.1796080716621419500.1231367054.1231367054.1231367054.1; __hgzo=144631658.0.10.1231367054; __h"
    "gzp=144631658; ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&Sb"
    "eprqRkcvengvba=633669350559478880&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlM"
    "Qo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9"
    "hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 643};
  str82 = (Str){"[glcr=fhozvg]", 13};
  str83 = (Str){"n.svryqOga,n.svryqOgaPnapry", 27};
  str84 = (Str){"n.svryqOgaPnapry", 16};
  str85 = (Str){"oyvpxchaxg", 10};
  str86 = (Str){"qvi.bow-nppbeqvba qg", 20};
  str87 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_nccf_wf&qg=1231367052227&uy"
    "=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231367052227&punaary=svz_zlfc"
    "npr_nccf-pnainf%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%3N%2S%2Scebsvyr.zlfcnpr.pbz%2SZbqhyrf%2SNcc"
    "yvpngvbaf%2SCntrf%2SPnainf.nfck&nq_glcr=grkg&rvq=6083027&rn=0&sez=1&tn_ivq=716357910.1231367056&"
    "tn_fvq=1231367056&tn_uvq=1387206491&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=1024&h_pq=24&h_"
    "gm=-480&h_uvf=2&h_wnin=gehr&h_acyht=7&h_azvzr=22", 527};
  str88 = (Str){"uggc://tbbtyrnqf.t.qbhoyrpyvpx.arg/cntrnq/nqf\?pyvrag=pn-svz_zlfcnpr_zlfcnpr-ubzrcntr_wf&qg=1231"
    "365851658&uy=ra&nqfnsr=uvtu&br=hgs8&ahz_nqf=4&bhgchg=wf&nqgrfg=bss&pbeeryngbe=1231365851658&puna"
    "ary=svz_zlfcnpr_ubzrcntr_abgybttrqva%2Psvz_zlfcnpr_aba_HTP%2Psvz_zlfcnpr_havgrq-fgngrf&hey=uggc%"
    "3N%2S%2Scebsvyrrqvg.zlfcnpr.pbz%2Svaqrk.psz&nq_glcr=grkg&rvq=6083027&rn=0&sez=0&tn_ivq=197982812"
    "9.1231365855&tn_fvq=1231365855&tn_uvq=2085229649&synfu=9.0.115&h_u=768&h_j=1024&h_nu=738&h_nj=10"
    "24&h_pq=24&h_gm=-480&h_uvf=2&h_wnin=gehr&h_acyht=7&h_azvzr=22", 540};
  str89 = (Str){"uggc://zfacbegny.112.2b7.arg/o/ff/zfacbegnyubzr/1/U.7-cqi-2/f55023338617756\?[NDO]&aqu=1&g=7%2S0"
    "%2S2009%2014%3N12%3N47%203%20480&af=zfacbegny&cntrAnzr=HF%20UCZFSGJ&t=uggc%3N%2S%2Sjjj.zfa.pbz%2"
    "S&f=0k0&p=43835816&x=A&oj=994&ou=634&uc=A&{2}&[NDR]", 242};
  str90 = (Str){"zrgn[anzr=nwnkHey]", 18};
  str91 = (Str){"anpuevpugra", 11};
  str92 = (Str){"b oS={'oT':1.1};x $8n(B){z(B!=o9)};x $S(B){O(!$8n(B))z A;O(B.4L)z'T';b S=7t B;O(S=='2P'&&B.p4){2"
    "3(B.7f){12 1:z'T';12 3:z/S/.2g(B.8M)\?'ox':'oh'}}O(S=='2P'||S=='x'){23(B.nE){12 2V:z'1O';12 7I:z"
    "'5a';12 18:z'4B'}O(7t B.I=='4F'){O(B.3u)z'pG';O(B.8e)z'1p'}}z S};x $2p(){b 4E={};Z(b v=0;v<1p.I;"
    "v++){Z(b X 1o 1p[v]){b nc=1p[v][X];b 6E=4E[X];O(6E&&$S(nc)=='2P'&&$S(6E)=='2P')4E[X]=$2p(6E,nc);"
    "17 4E[X]=nc}}z 4E};b $E=7p.E=x(){b 1d=1p;O(!1d[1])1d=[p,1d[0]];Z(b X 1o 1d[1])1d[0][X]=1d[1][X];"
    "z 1d[0]};b $4D=7p.pJ=x(){Z(b v=0,y=1p.I;v<y;v++){1p[v].E=x(1J){Z(b 1I 1o 1J){O(!p.1Y[1I])p.1Y[1I"
    "]=1J[1I];O(!p[1I])p[1I]=$4D.6C(1I)}}}};$4D.6C=x(1I){z x(L){z p.1Y[1I].3H(L,2V.1Y.nV.1F(1p,1))}};"
    "$4D(7F,2V,6J,nb);b 3l=x(B){B=B||{};B.E=$E;z B};b pK=Y 3l(H);b pZ=Y 3l(C);C.6f=C.35('6f')[0];x $2"
    "O(B){z!!(B||B===0)};x $5S(B,n8){z $8n(B)\?B:n8};x $7K(3c,1m){z 1q.na(1q.7K()*(1m-3c+1)+3c)};x $3"
    "N(){z Y 97().os()};x $4M(1U){pv(1U);pa(1U);z 1S};H.43=!!(C.5Z);O(H.nB)H.31=H[H.7q\?'py':'nL']=1r"
    ";17 O(C.9N&&!C.om&&!oy.oZ)H.pF=H.4Z=H[H.43\?'pt':'65']=1r;17 O(C.po!=1S)H.7J=1r;O(7t 5B=='o9'){b"
    " 5B=x(){};O(H.4Z)C.nd(\"pW\");5B.1Y=(H.4Z)\?H[\"[[oN.1Y]]\"]:{}}5B.1Y.4L=1r;O(H.nL)5s{C.oX(\"pp"
    "\",A,1r)}4K(r){};b 18=x(1X){b 63=x(){z(1p[0]!==1S&&p.1w&&$S(p.1w)=='x')\?p.1w.3H(p,1p):p};$E(63,"
    "p);63.1Y=1X;63.nE=18;z 63};18.1z=x(){};18.1Y={E:x(1X){b 7x=Y p(1S);Z(b X 1o 1X){b nC=7x[X];7x[X]"
    "=18.nY(nC,1X[X])}z Y 18(7x)},3d:x(){Z(b v=0,y=1p.I;v<y;v++)$E(p.1Y,1p[v])}};18.nY=x(2b,2n){O(2b&"
    "&2b!=2n){b S=$S(2n);O(S!=$S(2b))z 2n;23(S){12'x':b 7R=x(){p.1e=1p.8e.1e;z 2n.3H(p,1p)};7R.1e=2b;"
    "z 7R;12'2P':z $2p(2b,2n)}}z 2n};b 8o=Y 18({oQ:x(J){p.4w=p.4w||[];p.4w.1x(J);z p},7g:x(){O(p.4w&&"
    "p.4w.I)p.4w.9J().2x(10,p)},oP:x(){p.4w=[]}});b 2d=Y 18({1V:x(S,J){O(J!=18.1z){p.$19=p.$19||{};p."
    "$19[S]=p.$19[S]||[];p.$19[S].5j(J)}z p},1v:x(S,1d,2x){O(p.$19&&p.$19[S]){p.$19[S].1b(x(J){J.3n({"
    "'L':p,'2x':2x,'1p':1d})()},p)}z p},3M:x(S,J){O(p.$19&&p.$19[S])p.$19[S].2U(J);z p}});b 4v=Y 18({"
    "2H:x(){p.P=$2p.3H(1S,[p.P].E(1p));O(!p.1V)z p;Z(b 3O 1o p.P){O($S(p.P[3O]=='x')&&3O.2g(/^5P[N-M]"
    "/))p.1V(3O,p.P[3O])}z p}});2V.E({7y:x(J,L){Z(b v=0,w=p.I;v<w;v++)J.1F(L,p[v],v,p)},3s:x(J,L){b 5"
    "4=[];Z(b v=0,w=p.I;v<w;v++){O(J.1F(L,p[v],v,p))54.1x(p[v])}z 54},2X:x(J,L){b 54=[];Z(b v=0,w=p.I"
    ";v<w;v++)54[v]=J.1F(L,p[v],v,p);z 54},4i:x(J,L){Z(b v=0,w=p.I;v<w;v++){O(!J.1F(L,p[v],v,p))z A}z"
    " 1r},ob:x(J,L){Z(b v=0,w=p.I;v<w;v++){O(J.1F(L,p[v],v,p))z 1r}z A},3F:x(3u,15){b 3A=p.I;Z(b v=(1"
    "5<0)\?1q.1m(0,3A+15):15||0;v<3A;v++){O(p[v]===3u)z v}z-1},8z:x(1u,I){1u=1u||0;O(1u<0)1u=p.I+1u;I"
    "=I||(p.I-1u);b 89=[];Z(b v=0;v<I;v++)89[v]=p[1u++];z 89},2U:x(3u){b v=0;b 3A=p.I;6L(v<3A){O(p[v]"
    "===3u){p.6l(v,1);3A--}17{v++}}z p},1y:x(3u,15){z p.3F(3u,15)!=-1},oz:x(1C){b B={},I=1q.3c(p.I,1C"
    ".I);Z(b v=0;v<I;v++)B[1C[v]]=p[v];z B},E:x(1O){Z(b v=0,w=1O.I;v<w;v++)p.1x(1O[v]);z p},2p:x(1O){"
    "Z(b v=0,y=1O.I;v<y;v++)p.5j(1O[v]);z p},5j:x(3u){O(!p.1y(3u))p.1x(3u);z p},oc:x(){z p[$7K(0,p.I-"
    "1)]||A},7L:x(){z p[p.I-1]||A}});2V.1Y.1b=2V.1Y.7y;2V.1Y.2g=2V.1Y.1y;x $N(1O){z 2V.8z(1O)};x $1b("
    "3J,J,L){O(3J&&7t 3J.I=='4F'&&$S(3J)!='2P')2V.7y(3J,J,L);17 Z(b 1j 1o 3J)J.1F(L||3J,3J[1j],1j)};6"
    "J.E({2g:x(6b,2F){z(($S(6b)=='2R')\?Y 7I(6b,2F):6b).2g(p)},3p:x(){z 5K(p,10)},o4:x(){z 69(p)},7A:"
    "x(){z p.3y(/-D/t,x(2G){z 2G.7G(1).nW()})},9b:x(){z p.3y(/w[N-M]/t,x(2G){z(2G.7G(0)+'-'+2G.7G(1)."
    "5O())})},8V:x(){z p.3y(/\010[n-m]/t,x(2G){z 2G.nW()})},5L:x(){z p.3y(/^s+|s+$/t,'')},7j:x(){z p."
    "3y(/s{2,}/t,' ').5L()},5V:x(1O){b 1i=p.2G(/d{1,3}/t);z(1i)\?1i.5V(1O):A},5U:x(1O){b 3P=p.2G(/^#"
    "\?(w{1,2})(w{1,2})(w{1,2})$/);z(3P)\?3P.nV(1).5U(1O):A},1y:x(2R,f){z(f)\?(f+p+f).3F(f+2R+f)>-1:p"
    ".3F(2R)>-1},nX:x(){z p.3y(/([.*+\?^${}()|[]/\\])/t,'\\$1')}});2V.E({5V:x(1O){O(p.I<3)z A;O(p.I=="
    "4&&p[3]==0&&!1O)z'p5';b 3P=[];Z(b v=0;v<3;v++){b 52=(p[v]-0).4h(16);3P.1x((52.I==1)\?'0'+52:52)}"
    "z 1O\?3P:'#'+3P.2u('')},5U:x(1O){O(p.I!=3)z A;b 1i=[];Z(b v=0;v<3;v++){1i.1x(5K((p[v].I==1)\?p[v"
    "]+p[v]:p[v],16))}z 1O\?1i:'1i('+1i.2u(',')+')'}});7F.E({3n:x(P){b J=p;P=$2p({'L':J,'V':A,'1p':1S"
    ",'2x':A,'4s':A,'6W':A},P);O($2O(P.1p)&&$S(P.1p)!='1O')P.1p=[P.1p];z x(V){b 1d;O(P.V){V=V||H.V;1d"
    "=[(P.V===1r)\?V:Y P.V(V)];O(P.1p)1d.E(P.1p)}17 1d=P.1p||1p;b 3C=x(){z J.3H($5S(P", 4081};
  str93 = (Str){"hagreunyghat", 12};
  str94 = (Str){"ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba="
    "633669341278771470&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG"
    "1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZw"
    "VhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 340};
  str95 = (Str){"ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&Pbhagel=IIZ%3Q&SbeprqRkcvengvba="
    "633669350559478880&gvzrMbar=-8&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4tIzyyqlMQo3IhqUW5D29xMG"
    "1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDkBGVzGT9hM2y0qJEyCF0kZw"
    "VhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R%3Q", 340};
  str96 = (Str){"ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel"
    "=IIZ=&SbeprqRkcvengvba=633669341278771470&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4t"
    "IzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDk"
    "BGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 360};
  str97 = (Str){"ZFPhygher=VC=74.125.75.1&VCPhygher=ra-HF&CersreerqPhygher=ra-HF&CersreerqPhygherCraqvat=&Pbhagel"
    "=IIZ=&SbeprqRkcvengvba=633669350559478880&gvzrMbar=0&HFEYBP=DKWyLHAiMTH9AwHjWxAcqUx9GJ91oaEunJ4t"
    "IzyyqlMQo3IhqUW5D29xMG1IHlMQo3IhqUW5GzSgMG1Iozy0MJDtH3EuqTImWxEgLHAiMTH9BQN3WxkuqTy0qJEyCGZ3YwDk"
    "BGVzGT9hM2y0qJEyCF0kZwVhZQH3APMDo3A0LJkQo2EyCGx0ZQDmWyWyM2yiox5uoJH9D0R=", 360};
  str98 = (Str){"shapgvba (){Cuk.Nccyvpngvba.Frghc.Pber();Cuk.Nccyvpngvba.Frghc.Nwnk();Cuk.Nccyvpngvba.Frghc.Synf"
    "u();Cuk.Nccyvpngvba.Frghc.Zbqhyrf()}", 132};
}

/* ---- The benchmark blocks (runBlock0..11). ---- */
static int64_t runBlock0(void) {
  int64_t sum = 0;
    for (int i = 0; i < 525; i++) {
      sum += rx_exec_len(RX[0], &s0[i]);
    }
    for (int i = 0; i < 1844; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 525]);
      sum += rx_exec_len(RX[1], &s1[i]);
    }
    for (int i = 0; i < 739; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 2369]);
      sum += rx_replace_len(RX[2], &s2[i]);
    }
    for (int i = 0; i < 598; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 3108]);
      sum += rx_exec_len(RX[1], &s3[i]);
    }
    for (int i = 0; i < 454; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 3706]);
      sum += rx_exec_len(RX[1], &s4[i]);
    }
    for (int i = 0; i < 352; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 4160]);
      sum += rx_exec_len(RX[95], &s5[i]);
    }
    for (int i = 0; i < 312; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 4512]);
      sum += rx_exec_len(RX[3], &s6[i]);
    }
    for (int i = 0; i < 282; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 4824]);
      sum += rx_exec_len(RX[4], &s7[i]);
    }
    for (int i = 0; i < 177; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 5106]);
      sum += rx_replace_len(RX[5], &s8[i]);
    }
    for (int i = 0; i < 170; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 5283]);
      sum += rx_replace_len(RX[6], &s9[i]);
      sum += rx_exec_len(RX[7], &s10[i]);
    }
    for (int i = 0; i < 156; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 5453]);
      sum += rx_exec_len(RX[8], &s11[i]);
      sum += rx_exec_len(RX[8], &s12[i]);
    }
    for (int i = 0; i < 144; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 5609]);
      sum += rx_exec_len(RX[0], &s13[i]);
    }
    for (int i = 0; i < 139; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 5753]);
      sum += rx_replace_len(RX[6], &s14[i]);
      sum += rx_exec_len(RX[7], &s14[i]);
      sum += rx_exec_len(RX[9], &(Str){"", 0});
      sum += rx_exec_len(RX[96], &s15[i]);
    }
    for (int i = 0; i < 137; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 5892]);
      sum += rx_replace_len(RX[10], &s16[i]);
      sum += rx_replace_len(RX[97], &s16[i]);
      sum += rx_replace_len(RX[11], &s17[i]);
    }
    for (int i = 0; i < 117; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 6029]);
      sum += rx_replace_len(RX[2], &s18[i]);
    }
    for (int i = 0; i < 95; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 6146]);
      sum += rx_exec_len(RX[98], &s19[i]);
    }
    for (int i = 0; i < 93; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 6241]);
      sum += rx_replace_len(RX[12], &s20[i]);
      sum += rx_exec_len(RX[13], &s20[i]);
    }
    for (int i = 0; i < 92; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 6334]);
      sum += rx_replace_len(RX[99], &s21[i]);
    }
    for (int i = 0; i < 85; i++) {
      sum += rx_exec_len(RX[0], &s0[i + 6426]);
      sum += rx_replace_len(RX[14], &s22[i]);
      sum += rx_replace_len(RX[15], &s22[i]);
      sum += rx_replace_len(RX[12], &s23[i]);
      sum += rx_replace_len(RX[14], &s24[i]);
      sum += rx_replace_len(RX[15], &s24[i]);
      sum += rx_exec_len(RX[16], &s25[i]);
      sum += rx_exec_len(RX[13], &s23[i]);
    }
  return sum;
}

static int64_t runBlock1(void) {
  int64_t sum = 0;
    for (int i = 0; i < 78; i++) {
      sum += rx_exec_len(RX[8], &s26[i]);
      sum += rx_replace_len(RX[100], &s27[i]);
      sum += rx_replace_len(RX[101], &s28[i]);
      sum += rx_replace_len(RX[17], &s29[i]);
      sum += rx_replace_len(RX[17], &s30[i]);
      sum += rx_exec_len(RX[8], &s31[i]);
      sum += rx_exec_len(RX[8], &s32[i]);
      sum += rx_exec_len(RX[8], &s33[i]);
      sum += rx_exec_len(RX[8], &s34[i]);
      sum += rx_exec_len(RX[8], &s35[i]);
      sum += rx_exec_len(RX[8], &s36[i]);
      sum += rx_exec_len(RX[8], &s37[i]);
      sum += rx_exec_len(RX[8], &s38[i]);
      sum += rx_exec_len(RX[8], &s39[i]);
      sum += rx_exec_len(RX[102], &s15[i]);
      sum += rx_exec_len(RX[3], &s41[i]);
      sum += rx_exec_len(RX[0], &s42[i]);
      sum += rx_exec_len(RX[0], &s43[i]);
    }
    for (int i = 0; i < 77; i++) {
      sum += rx_replace_len(RX[12], &s44[i]);
      sum += rx_exec_len(RX[13], &s44[i]);
    }
    for (int i = 0; i < 73; i++) {
      /* s46 has 72 variants; the JS reads s46[72] as undefined, which re.exec coerces to "undefined". */
      static Str undef_str = {"undefined", 9};
      sum += rx_replace_len(RX[18], &s45[i]);
      sum += rx_exec_len(RX[1], i < 72 ? &s46[i] : &undef_str);
    }
    for (int i = 0; i < 70; i++) {
      sum += rx_exec_len(RX[19], &(Str){"", 0});
      sum += rx_replace_len(RX[11], &s47[i]);
      sum += rx_replace_len(RX[103], &s48[i]);
      sum += rx_replace_len(RX[104], &s49[i]);
      sum += rx_replace_len(RX[105], &s50[i]);
      sum += rx_replace_len(RX[106], &s51[i]);
      sum += rx_split_len(RX[20], &s52[i]);
      sum += rx_exec_len(RX[21], &s53[i]);
    }
    for (int i = 0; i < 68; i++) {
      sum += rx_exec_len(RX[1], &s54[i]);
      sum += rx_exec_len(RX[107], &s15[i]);
      sum += rx_exec_len(RX[108], &s15[i]);
      sum += rx_exec_len(RX[109], &s15[i]);
      sum += rx_exec_len(RX[21], &s55[i]);
    }
    for (int i = 0; i < 44; i++) {
      sum += rx_exec_len(RX[16], &s56[i]);
      sum += rx_replace_len(RX[12], &s57[i]);
      sum += rx_exec_len(RX[13], &s57[i]);
    }
  return sum;
}

static int64_t runBlock2(void) {
  int64_t sum = 0;
    for (int i = 0; i < 40; i++) {
      sum += rx_replace_len(RX[14], &s57a[i]);
      sum += rx_replace_len(RX[15], &s57a[i]);
    }
    for (int i = 0; i < 39; i++) {
      sum += rx_replace_len(RX[110], &s58[i]);
      sum += rx_exec_len(RX[3], &s59[i]);
      sum += rx_exec_len(RX[3], &s60[i]);
      sum += rx_exec_len(RX[22], &(Str){"HVYvaxOhggba", 12});
      sum += rx_exec_len(RX[22], &(Str){"HVYvaxOhggba_E", 14});
      sum += rx_exec_len(RX[22], &(Str){"HVYvaxOhggba_EJ", 15});
      sum += rx_exec_len(RX[22], &(Str){"zrah_ybtva_pbagnvare", 20});
      sum += rx_exec_len(RX[111], &(Str){"vachgcnffjbeq", 13});
    }
    for (int i = 0; i < 37; i++) {
      sum += rx_exec_len(RX[8], &(Str){"111soqs57qo8o8480qo18sor2011r3n591q7s6s37r120904", 48});
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669315660164980", 35});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=111soqs57qo8o8480qo18sor2011r3n591q7s6s37r120904", 60});
    }
    for (int i = 0; i < 35; i++) {
      sum += rx_replace_len(RX[14], &(Str){"puvyq p1 svefg", 14});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p1 svefg", 14});
      sum += rx_replace_len(RX[14], &(Str){"sylbhg pybfrq", 13});
      sum += rx_replace_len(RX[15], &(Str){"sylbhg pybfrq", 13});
    }
    for (int i = 0; i < 34; i++) {
      sum += rx_exec_len(RX[19], &(Str){"gno2", 4});
      sum += rx_exec_len(RX[19], &(Str){"gno3", 4});
      sum += rx_exec_len(RX[8], &(Str){"44132r503660", 12});
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669316860113296", 35});
      sum += rx_exec_len(RX[8], &(Str){"AFP_zp_dfctwzs-aowb_80=44132r503660", 35});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=s6r4579npn4rn2135s904r0s75pp1o5334p6s6pospo12696", 60});
      sum += rx_exec_len(RX[8], &(Str){"s6r4579npn4rn2135s904r0s75pp1o5334p6s6pospo12696", 48});
    }
    for (int i = 0; i < 31; i++) {
      sum += rx_exec_len(RX[112], &s15[i]);
      sum += rx_replace_len(RX[23], &s61[i]);
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669358527244818", 35});
      sum += rx_exec_len(RX[8], &(Str){"VC=66.249.85.130", 16});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=s15q53p9n372sn76npr13o271n4s3p5r29p235746p908p58", 60});
      sum += rx_exec_len(RX[8], &(Str){"s15q53p9n372sn76npr13o271n4s3p5r29p235746p908p58", 48});
      sum += rx_exec_len(RX[24], &s61[i]);
    }
    for (int i = 0; i < 30; i++) {
      sum += rx_replace_len(RX[6], &s65[i]);
      sum += rx_exec_len(RX[113], &s66[i]);
      sum += rx_exec_len(RX[7], &s65[i]);
    }
    for (int i = 0; i < 28; i++) {
      sum += rx_replace_len(RX[23], &s62[i]);
      sum += rx_replace_len(RX[25], &s63[i]);
      sum += rx_replace_len(RX[12], &s63[i]);
      sum += rx_exec_len(RX[26], &s64[i]);
    }
  return sum;
}

static int64_t runBlock3(void) {
  int64_t sum = 0;
    for (int i = 0; i < 23; i++) {
      sum += rx_replace_len(RX[114], &s67[i]);
      sum += rx_replace_len(RX[27], &s68[i]);
      sum += rx_replace_len(RX[27], &s69[i]);
    }
    for (int i = 0; i < 22; i++) {
      sum += rx_replace_len(RX[14], &(Str){"unaqyr", 6});
      sum += rx_replace_len(RX[15], &(Str){"unaqyr", 6});
      sum += rx_replace_len(RX[14], &(Str){"yvar", 4});
      sum += rx_replace_len(RX[15], &(Str){"yvar", 4});
      sum += rx_replace_len(RX[14], &(Str){"cnerag puebzr6 fvatyr1 gno", 26});
      sum += rx_replace_len(RX[15], &(Str){"cnerag puebzr6 fvatyr1 gno", 26});
      sum += rx_replace_len(RX[14], &(Str){"fyvqre", 6});
      sum += rx_replace_len(RX[15], &(Str){"fyvqre", 6});
      sum += rx_exec_len(RX[28], &(Str){"", 0});
    }
    for (int i = 0; i < 21; i++) {
      sum += rx_replace_len(RX[12], &s70[i]);
      sum += rx_exec_len(RX[13], &s70[i]);
    }
    for (int i = 0; i < 20; i++) {
      sum += rx_replace_len(RX[29], &s71[i]);
      sum += rx_replace_len(RX[30], &s71[i]);
      sum += rx_exec_len(RX[19], &(Str){"ynfg", 4});
      sum += rx_exec_len(RX[19], &(Str){"ba svefg", 8});
      sum += rx_exec_len(RX[8], &s72[i]);
    }
    for (int i = 0; i < 18; i++) {
      sum += rx_exec_len(RX[31], &s73[i]);
      sum += rx_split_len(RX[32], &s74[i]);
      sum += rx_split_len(RX[32], &s75[i]);
      sum += rx_replace_len(RX[33], &s76[i]);
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231363570", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231363570.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.3426875219718084000.1231363570.1231363570.1231363570.1", 64});
      sum += rx_exec_len(RX[8], &str13);
      sum += rx_exec_len(RX[8], &str14);
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.3426875219718084000.1231363570.1231363570.1231363570.1", 71});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231363570", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231363570.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
      sum += rx_exec_len(RX[34], &s74[i]);
      sum += rx_exec_len(RX[34], &s75[i]);
    }
    for (int i = 0; i < 17; i++) {
      (void)rx_match_all(RX[115], &s15[i]);
      (void)rx_match_all(RX[116], &s15[i]);
      sum += rx_split_len(RX[32], &str15);
      sum += rx_split_len(RX[32], &str16);
      sum += rx_replace_len(RX[14], &(Str){"ohggba", 6});
      sum += rx_replace_len(RX[15], &(Str){"ohggba", 6});
      sum += rx_replace_len(RX[14], &(Str){"puvyq p1 svefg sylbhg pybfrq", 28});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p1 svefg sylbhg pybfrq", 28});
      sum += rx_replace_len(RX[14], &(Str){"pvgvrf", 6});
      sum += rx_replace_len(RX[15], &(Str){"pvgvrf", 6});
      sum += rx_replace_len(RX[14], &(Str){"pybfrq", 6});
      sum += rx_replace_len(RX[15], &(Str){"pybfrq", 6});
      sum += rx_replace_len(RX[14], &(Str){"qry", 3});
      sum += rx_replace_len(RX[15], &(Str){"qry", 3});
      sum += rx_replace_len(RX[14], &(Str){"uqy_zba", 7});
      sum += rx_replace_len(RX[15], &(Str){"uqy_zba", 7});
      sum += rx_replace_len(RX[33], &s77[i]);
      sum += rx_replace_len(RX[117], &s78[i]);
      sum += rx_replace_len(RX[118], &s78[i]);
      sum += rx_replace_len(RX[119], &s78[i]);
      sum += rx_replace_len(RX[35], &s78[i]);
      sum += rx_replace_len(RX[14], &(Str){"yvaxyvfg16", 10});
      sum += rx_replace_len(RX[15], &(Str){"yvaxyvfg16", 10});
      sum += rx_replace_len(RX[14], &(Str){"zvahf", 5});
      sum += rx_replace_len(RX[15], &(Str){"zvahf", 5});
      sum += rx_replace_len(RX[14], &(Str){"bcra", 4});
      sum += rx_replace_len(RX[15], &(Str){"bcra", 4});
      sum += rx_replace_len(RX[14], &(Str){"cnerag puebzr5 fvatyr1 ps NU", 28});
      sum += rx_replace_len(RX[15], &(Str){"cnerag puebzr5 fvatyr1 ps NU", 28});
      sum += rx_replace_len(RX[14], &(Str){"cynlre", 6});
      sum += rx_replace_len(RX[15], &(Str){"cynlre", 6});
      sum += rx_replace_len(RX[14], &(Str){"cyhf", 4});
      sum += rx_replace_len(RX[15], &(Str){"cyhf", 4});
      sum += rx_replace_len(RX[14], &(Str){"cb_uqy", 6});
      sum += rx_replace_len(RX[15], &(Str){"cb_uqy", 6});
      sum += rx_replace_len(RX[14], &(Str){"hyJVzt", 6});
      sum += rx_replace_len(RX[15], &(Str){"hyJVzt", 6});
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231363638", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231363638.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.965867047679498800.1231363638.1231363638.1231363638.1", 63});
      sum += rx_exec_len(RX[8], &(Str){"4413268q3660", 12});
      sum += rx_exec_len(RX[8], &(Str){"4ss747o77904333q374or84qrr1s9r0nprp8r5q81534o94n", 48});
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669321699093060", 35});
      sum += rx_exec_len(RX[8], &(Str){"VC=74.125.75.20", 15});
      sum += rx_exec_len(RX[8], &str19);
      sum += rx_exec_len(RX[8], &str20);
      sum += rx_exec_len(RX[8], &(Str){"AFP_zp_tfwsbrg-aowb_80=4413268q3660", 35});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=4ss747o77904333q374or84qrr1s9r0nprp8r5q81534o94n", 60});
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.965867047679498800.1231363638.1231363638.1231363638.1", 70});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231363638", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231363638.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
      sum += rx_exec_len(RX[34], &str15);
      sum += rx_exec_len(RX[34], &str16);
    }
  return sum;
}

static int64_t runBlock4(void) {
  int64_t sum = 0;
    for (int i = 0; i < 16; i++) {
      sum += rx_replace_len(RX[120], &(Str){"", 0});
      sum += rx_exec_len(RX[121], &(Str){"npgvir", 6});
      sum += rx_exec_len(RX[122], &s15[i]);
      sum += rx_exec_len(RX[36], &(Str){"glcr", 4});
      sum += rx_exec_len(RX[123], &s15[i]);
      sum += rx_exec_len(RX[124], &s15[i]);
    }
    for (int i = 0; i < 15; i++) {
      sum += rx_split_len(RX[32], &s79[i]);
      sum += rx_split_len(RX[32], &s80[i]);
      sum += rx_replace_len(RX[12], &(Str){"uggc://ohyyrgvaf.zlfcnpr.pbz/vaqrk.psz", 38});
      sum += rx_replace_len(RX[33], &s81[i]);
      sum += rx_replace_len(RX[37], &(Str){"yv", 2});
      sum += rx_replace_len(RX[18], &(Str){"yv", 2});
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231367822", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231367822.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.4127520630321984500.1231367822.1231367822.1231367822.1", 64});
      sum += rx_exec_len(RX[8], &str24);
      sum += rx_exec_len(RX[8], &str25);
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.4127520630321984500.1231367822.1231367822.1231367822.1", 71});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231367822", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231367822.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
      sum += rx_exec_len(RX[34], &s79[i]);
      sum += rx_exec_len(RX[34], &s80[i]);
      sum += rx_exec_len(RX[125], &s82[i]);
      sum += rx_exec_len(RX[13], &(Str){"uggc://ohyyrgvaf.zlfcnpr.pbz/vaqrk.psz", 38});
      sum += rx_exec_len(RX[38], &(Str){"yv", 2});
    }
    for (int i = 0; i < 14; i++) {
      sum += rx_replace_len(RX[18], &(Str){"", 0});
      sum += rx_replace_len(RX[126], &(Str){"9.0  e115", 9});
      sum += rx_replace_len(RX[127], &(Str){"Funer guvf tnqtrg", 17});
      sum += rx_replace_len(RX[128], &(Str){"Funer guvf tnqtrg", 17});
      sum += rx_replace_len(RX[39], &(Str){"Funer guvf tnqtrg", 17});
      sum += rx_replace_len(RX[12], &(Str){"uggc://cebsvyrrqvg.zlfcnpr.pbz/vaqrk.psz", 40});
      sum += rx_replace_len(RX[40], &(Str){"grnfre", 6});
      sum += rx_replace_len(RX[41], &(Str){"grnfre", 6});
      sum += rx_replace_len(RX[42], &(Str){"grnfre", 6});
      sum += rx_replace_len(RX[43], &(Str){"grnfre", 6});
      sum += rx_replace_len(RX[44], &(Str){"grnfre", 6});
      sum += rx_replace_len(RX[45], &(Str){"grnfre", 6});
      sum += rx_replace_len(RX[46], &(Str){"grnfre", 6});
      sum += rx_replace_len(RX[47], &(Str){"grnfre", 6});
      sum += rx_replace_len(RX[48], &(Str){"grnfre", 6});
      sum += rx_exec_len(RX[16], &(Str){"znetva-gbc", 10});
      sum += rx_exec_len(RX[16], &(Str){"cbfvgvba", 8});
      sum += rx_exec_len(RX[19], &(Str){"gno1", 4});
      sum += rx_exec_len(RX[9], &(Str){"qz", 2});
      sum += rx_exec_len(RX[9], &(Str){"qg", 2});
      sum += rx_exec_len(RX[9], &(Str){"zbqobk", 6});
      sum += rx_exec_len(RX[9], &(Str){"zbqobkva", 8});
      sum += rx_exec_len(RX[9], &(Str){"zbqgvgyr", 8});
      sum += rx_exec_len(RX[13], &(Str){"uggc://cebsvyrrqvg.zlfcnpr.pbz/vaqrk.psz", 40});
      sum += rx_exec_len(RX[26], &(Str){"/vt/znvytnqtrg", 14});
      sum += rx_exec_len(RX[49], &(Str){"glcr", 4});
    }
  return sum;
}

static int64_t runBlock5(void) {
  int64_t sum = 0;
    for (int i = 0; i < 13; i++) {
      sum += rx_replace_len(RX[14], &(Str){"purpx", 5});
      sum += rx_replace_len(RX[15], &(Str){"purpx", 5});
      sum += rx_replace_len(RX[14], &(Str){"pvgl", 4});
      sum += rx_replace_len(RX[15], &(Str){"pvgl", 4});
      sum += rx_replace_len(RX[14], &(Str){"qrpe fyvqrgrkg", 14});
      sum += rx_replace_len(RX[15], &(Str){"qrpe fyvqrgrkg", 14});
      sum += rx_replace_len(RX[14], &(Str){"svefg fryrpgrq", 14});
      sum += rx_replace_len(RX[15], &(Str){"svefg fryrpgrq", 14});
      sum += rx_replace_len(RX[14], &(Str){"uqy_rag", 7});
      sum += rx_replace_len(RX[15], &(Str){"uqy_rag", 7});
      sum += rx_replace_len(RX[14], &(Str){"vape fyvqrgrkg", 14});
      sum += rx_replace_len(RX[15], &(Str){"vape fyvqrgrkg", 14});
      sum += rx_replace_len(RX[5], &(Str){"vachggrkg QBZPbageby_cynprubyqre", 32});
      sum += rx_replace_len(RX[14], &(Str){"cnerag puebzr6 fvatyr1 gno fryrpgrq", 35});
      sum += rx_replace_len(RX[15], &(Str){"cnerag puebzr6 fvatyr1 gno fryrpgrq", 35});
      sum += rx_replace_len(RX[14], &(Str){"cb_guz", 6});
      sum += rx_replace_len(RX[15], &(Str){"cb_guz", 6});
      sum += rx_replace_len(RX[14], &(Str){"fhozvg", 6});
      sum += rx_replace_len(RX[15], &(Str){"fhozvg", 6});
      sum += rx_exec_len(RX[50], &(Str){"", 0});
      sum += rx_exec_len(RX[129], &s15[i]);
      sum += rx_exec_len(RX[130], &s15[i]);
    }
    for (int i = 0; i < 12; i++) {
      sum += rx_replace_len(RX[131], &(Str){"${cebg}://${ubfg}${cngu}/${dz}", 30});
      sum += rx_replace_len(RX[40], &(Str){"1", 1});
      sum += rx_replace_len(RX[10], &(Str){"1", 1});
      sum += rx_replace_len(RX[51], &(Str){"1", 1});
      sum += rx_replace_len(RX[52], &(Str){"1", 1});
      sum += rx_replace_len(RX[53], &(Str){"1", 1});
      sum += rx_replace_len(RX[39], &(Str){"1", 1});
      sum += rx_replace_len(RX[54], &(Str){"1", 1});
      sum += rx_replace_len(RX[132], &(Str){"9.0  e115", 9});
      sum += rx_replace_len(RX[133], &(Str){"9.0  e115", 9});
      sum += rx_replace_len(RX[55], &(Str){"<!-- ${nqiHey} -->", 18});
      sum += rx_replace_len(RX[55], &(Str){"<fpevcg glcr=\"grkg/wninfpevcg\" fep=\"${nqiHey}\"></fpevcg>", 56});
      sum += rx_replace_len(RX[134], &s21[i]);
      sum += rx_replace_len(RX[30], &(Str){"tzk%2Subzrcntr%2Sfgneg%2Sqr%2S", 30});
      sum += rx_replace_len(RX[30], &(Str){"tzk", 3});
      sum += rx_replace_len(RX[135], &(Str){"uggc://${ubfg}${cngu}/${dz}", 27});
      sum += rx_replace_len(RX[56], &(Str){"uggc://nqpyvrag.hvzfrei.arg${cngu}/${dz}", 40});
      sum += rx_replace_len(RX[136], &(Str){"uggc://nqpyvrag.hvzfrei.arg/wf.at/${dz}", 39});
      sum += rx_replace_len(RX[29], &(Str){"frpgvba", 7});
      sum += rx_replace_len(RX[30], &(Str){"frpgvba", 7});
      sum += rx_replace_len(RX[29], &(Str){"fvgr", 4});
      sum += rx_replace_len(RX[30], &(Str){"fvgr", 4});
      sum += rx_replace_len(RX[29], &(Str){"fcrpvny", 7});
      sum += rx_replace_len(RX[30], &(Str){"fcrpvny", 7});
      sum += rx_exec_len(RX[36], &(Str){"anzr", 4});
      sum += rx_exec_len(RX[137], &(Str){"9.0  e115", 9});
    }
  return sum;
}

static int64_t runBlock6(void) {
  int64_t sum = 0;
    for (int i = 0; i < 11; i++) {
      sum += rx_replace_len(RX[138], &s83[i]);
      sum += rx_replace_len(RX[57], &s83[i]);
      sum += rx_replace_len(RX[58], &s84[i]);
      sum += rx_replace_len(RX[59], &s85[i]);
      sum += rx_replace_len(RX[139], &s86[i]);
      sum += rx_replace_len(RX[140], &s86[i]);
      sum += rx_replace_len(RX[141], &s86[i]);
      sum += rx_replace_len(RX[142], &s86[i]);
      sum += rx_replace_len(RX[143], &s86[i]);
      sum += rx_replace_len(RX[144], &s86[i]);
      sum += rx_replace_len(RX[145], &s86[i]);
      sum += rx_replace_len(RX[146], &s87[i]);
      sum += rx_replace_len(RX[147], &s88[i]);
      sum += rx_replace_len(RX[148], &s89[i]);
      sum += rx_exec_len(RX[149], &s15[i]);
      sum += rx_exec_len(RX[150], &s83[i]);
      sum += rx_exec_len(RX[151], &s84[i]);
      sum += rx_exec_len(RX[152], &s84[i]);
      sum += rx_exec_len(RX[153], &s84[i]);
      sum += rx_exec_len(RX[154], &s84[i]);
      sum += rx_exec_len(RX[155], &s84[i]);
      sum += rx_exec_len(RX[156], &s84[i]);
      sum += rx_exec_len(RX[58], &s84[i]);
      sum += rx_exec_len(RX[157], &s85[i]);
      sum += rx_exec_len(RX[158], &s85[i]);
      sum += rx_exec_len(RX[59], &s85[i]);
      sum += rx_exec_len(RX[159], &s83[i]);
      sum += rx_exec_len(RX[160], &s86[i]);
      sum += rx_exec_len(RX[161], &s86[i]);
      sum += rx_exec_len(RX[162], &s86[i]);
      sum += rx_exec_len(RX[163], &s86[i]);
      sum += rx_exec_len(RX[164], &s83[i]);
      sum += rx_exec_len(RX[57], &s83[i]);
      sum += rx_exec_len(RX[165], &s84[i]);
      sum += rx_exec_len(RX[166], &s84[i]);
      sum += rx_exec_len(RX[167], &s84[i]);
      sum += rx_exec_len(RX[168], &s84[i]);
      sum += rx_exec_len(RX[169], &s84[i]);
      sum += rx_exec_len(RX[8], &(Str){"473qq1rs0n2r70q9qo1pq48n021s9468ron90nps048p4p29", 48});
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669325184628362", 35});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=473qq1rs0n2r70q9qo1pq48n021s9468ron90nps048p4p29", 60});
      sum += rx_exec_len(RX[170], &s15[i]);
    }
    for (int i = 0; i < 10; i++) {
      sum += rx_replace_len(RX[171], &(Str){" bss", 4});
      sum += rx_replace_len(RX[172], &s90[i]);
      sum += rx_replace_len(RX[173], &s90[i]);
      sum += rx_replace_len(RX[174], &s90[i]);
      sum += rx_replace_len(RX[175], &s90[i]);
      sum += rx_replace_len(RX[176], &s90[i]);
      sum += rx_replace_len(RX[56], &s90[i]);
      sum += rx_replace_len(RX[177], &s90[i]);
      sum += rx_replace_len(RX[178], &s90[i]);
      sum += rx_replace_len(RX[29], &(Str){"nqfvmr", 6});
      sum += rx_replace_len(RX[30], &(Str){"nqfvmr", 6});
      sum += rx_replace_len(RX[179], &(Str){"uggc://${2}${3}${4}${5}", 23});
      sum += rx_replace_len(RX[180], &(Str){"uggc://wf.hv-cbegny.qr${3}${4}${5}", 34});
      sum += rx_replace_len(RX[40], &(Str){"arjf", 4});
      sum += rx_replace_len(RX[41], &(Str){"arjf", 4});
      sum += rx_replace_len(RX[42], &(Str){"arjf", 4});
      sum += rx_replace_len(RX[43], &(Str){"arjf", 4});
      sum += rx_replace_len(RX[44], &(Str){"arjf", 4});
      sum += rx_replace_len(RX[45], &(Str){"arjf", 4});
      sum += rx_replace_len(RX[46], &(Str){"arjf", 4});
      sum += rx_replace_len(RX[47], &(Str){"arjf", 4});
      sum += rx_replace_len(RX[48], &(Str){"arjf", 4});
      sum += rx_exec_len(RX[181], &str35);
      sum += rx_exec_len(RX[60], &(Str){" ", 1});
      sum += rx_exec_len(RX[60], &(Str){" bss", 4});
      sum += rx_exec_len(RX[60], &(Str){"", 0});
      sum += rx_exec_len(RX[19], &(Str){" ", 1});
      sum += rx_exec_len(RX[19], &(Str){"svefg ba", 8});
      sum += rx_exec_len(RX[19], &(Str){"ynfg vtaber", 11});
      sum += rx_exec_len(RX[19], &(Str){"ba", 2});
      sum += rx_exec_len(RX[9], &(Str){"scnq so ", 8});
      sum += rx_exec_len(RX[9], &(Str){"zrqvgobk", 8});
      sum += rx_exec_len(RX[9], &(Str){"hsgy", 4});
      sum += rx_exec_len(RX[9], &(Str){"lhv-h", 5});
      sum += rx_exec_len(RX[182], &s15[i]);
      sum += rx_exec_len(RX[61], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/onfr.wf", 51});
      sum += rx_exec_len(RX[62], &(Str){"#Ybtva_rznvy", 12});
    }
  return sum;
}

static int64_t runBlock7(void) {
  int64_t sum = 0;
    for (int i = 0; i < 9; i++) {
      sum += rx_replace_len(RX[40], &(Str){"0", 1});
      sum += rx_replace_len(RX[10], &(Str){"0", 1});
      sum += rx_replace_len(RX[51], &(Str){"0", 1});
      sum += rx_replace_len(RX[52], &(Str){"0", 1});
      sum += rx_replace_len(RX[53], &(Str){"0", 1});
      sum += rx_replace_len(RX[39], &(Str){"0", 1});
      sum += rx_replace_len(RX[54], &(Str){"0", 1});
      sum += rx_replace_len(RX[40], &(Str){"Lrf", 3});
      sum += rx_replace_len(RX[10], &(Str){"Lrf", 3});
      sum += rx_replace_len(RX[51], &(Str){"Lrf", 3});
      sum += rx_replace_len(RX[52], &(Str){"Lrf", 3});
      sum += rx_replace_len(RX[53], &(Str){"Lrf", 3});
      sum += rx_replace_len(RX[39], &(Str){"Lrf", 3});
      sum += rx_replace_len(RX[54], &(Str){"Lrf", 3});
    }
    for (int i = 0; i < 8; i++) {
      sum += rx_replace_len(RX[63], &(Str){"Pybfr {0}", 9});
      sum += rx_replace_len(RX[63], &(Str){"Bcra {0}", 8});
      sum += rx_split_len(RX[32], &s91[i]);
      sum += rx_split_len(RX[32], &s92[i]);
      sum += rx_replace_len(RX[14], &(Str){"puvyq p1 svefg gnournqref", 25});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p1 svefg gnournqref", 25});
      sum += rx_replace_len(RX[14], &(Str){"uqy_fcb", 7});
      sum += rx_replace_len(RX[15], &(Str){"uqy_fcb", 7});
      sum += rx_replace_len(RX[14], &(Str){"uvag", 4});
      sum += rx_replace_len(RX[15], &(Str){"uvag", 4});
      sum += rx_replace_len(RX[33], &s93[i]);
      sum += rx_replace_len(RX[14], &(Str){"yvfg", 4});
      sum += rx_replace_len(RX[15], &(Str){"yvfg", 4});
      sum += rx_replace_len(RX[30], &(Str){"at_bhgre", 8});
      sum += rx_replace_len(RX[14], &(Str){"cnerag puebzr5 qbhoyr2 NU", 25});
      sum += rx_replace_len(RX[15], &(Str){"cnerag puebzr5 qbhoyr2 NU", 25});
      sum += rx_replace_len(RX[14], &(Str){"cnerag puebzr5 dhnq5 ps NU osyvax zbarl", 39});
      sum += rx_replace_len(RX[15], &(Str){"cnerag puebzr5 dhnq5 ps NU osyvax zbarl", 39});
      sum += rx_replace_len(RX[14], &(Str){"cnerag puebzr6 fvatyr1", 22});
      sum += rx_replace_len(RX[15], &(Str){"cnerag puebzr6 fvatyr1", 22});
      sum += rx_replace_len(RX[14], &(Str){"cb_qrs", 6});
      sum += rx_replace_len(RX[15], &(Str){"cb_qrs", 6});
      sum += rx_replace_len(RX[14], &(Str){"gnopbagrag", 10});
      sum += rx_replace_len(RX[15], &(Str){"gnopbagrag", 10});
      sum += rx_replace_len(RX[30], &(Str){"iv_svefg_gvzr", 13});
      sum += rx_exec_len(RX[183], &(Str){"cntrf.ronl.pbz", 14});
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231364074", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231364074.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.2294274870215848400.1231364074.1231364074.1231364074.1", 64});
      sum += rx_exec_len(RX[8], &(Str){"4413241q3660", 12});
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669357391353591", 35});
      sum += rx_exec_len(RX[8], &str39);
      sum += rx_exec_len(RX[8], &str40);
      sum += rx_exec_len(RX[8], &(Str){"AFP_zp_kkk-gdzogv_80=4413241q3660", 33});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=p98s8o9q42nr21or1r61pqorn1n002nsss569635984s6qp7", 60});
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.2294274870215848400.1231364074.1231364074.1231364074.1", 71});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231364074", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231364074.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
      sum += rx_exec_len(RX[8], &(Str){"p98s8o9q42nr21or1r61pqorn1n002nsss569635984s6qp7", 48});
      sum += rx_exec_len(RX[34], &s91[i]);
      sum += rx_exec_len(RX[34], &s92[i]);
    }
  return sum;
}

static int64_t runBlock8(void) {
  int64_t sum = 0;
    for (int i = 0; i < 7; i++) {
      (void)rx_match_all(RX[184], &s21[i]);
      sum += rx_replace_len(RX[64], &(Str){"nsgre", 5});
      sum += rx_replace_len(RX[64], &(Str){"orsber", 6});
      sum += rx_replace_len(RX[64], &(Str){"obggbz", 6});
      sum += rx_replace_len(RX[65], &(Str){"ohvygva_jrngure.kzy", 19});
      sum += rx_replace_len(RX[37], &(Str){"ohggba", 6});
      sum += rx_replace_len(RX[18], &(Str){"ohggba", 6});
      sum += rx_replace_len(RX[65], &(Str){"qngrgvzr.kzy", 12});
      sum += rx_replace_len(RX[65], &(Str){"uggc://eff.paa.pbz/eff/paa_gbcfgbevrf.eff", 41});
      sum += rx_replace_len(RX[37], &(Str){"vachg", 5});
      sum += rx_replace_len(RX[18], &(Str){"vachg", 5});
      sum += rx_replace_len(RX[64], &(Str){"vafvqr", 6});
      sum += rx_replace_len(RX[27], &(Str){"cbvagre", 7});
      sum += rx_replace_len(RX[185], &(Str){"cbfvgvba", 8});
      sum += rx_replace_len(RX[27], &(Str){"gbc", 3});
      sum += rx_replace_len(RX[64], &(Str){"gbc", 3});
      sum += rx_replace_len(RX[37], &(Str){"hy", 2});
      sum += rx_replace_len(RX[18], &(Str){"hy", 2});
      sum += rx_replace_len(RX[37], &str26);
      sum += rx_replace_len(RX[18], &str26);
      sum += rx_replace_len(RX[65], &(Str){"lbhghor_vtbbtyr/i2/lbhghor.kzy", 30});
      sum += rx_replace_len(RX[27], &(Str){"m-vaqrk", 7});
      sum += rx_exec_len(RX[186], &str26);
      sum += rx_exec_len(RX[16], &(Str){"urvtug", 6});
      sum += rx_exec_len(RX[16], &(Str){"znetvaGbc", 9});
      sum += rx_exec_len(RX[16], &(Str){"jvqgu", 5});
      sum += rx_exec_len(RX[19], &(Str){"gno0 svefg ba", 13});
      sum += rx_exec_len(RX[19], &(Str){"gno0 ba", 7});
      sum += rx_exec_len(RX[19], &(Str){"gno4 ynfg", 9});
      sum += rx_exec_len(RX[19], &(Str){"gno4", 4});
      sum += rx_exec_len(RX[19], &(Str){"gno5", 4});
      sum += rx_exec_len(RX[19], &(Str){"gno6", 4});
      sum += rx_exec_len(RX[19], &(Str){"gno7", 4});
      sum += rx_exec_len(RX[19], &(Str){"gno8", 4});
      sum += rx_exec_len(RX[187], &s15[i]);
      sum += rx_exec_len(RX[188], &s15[i]);
      sum += rx_exec_len(RX[189], &s15[i]);
      sum += rx_exec_len(RX[190], &(Str){"YV", 2});
      sum += rx_exec_len(RX[38], &(Str){"ohggba", 6});
      sum += rx_exec_len(RX[38], &(Str){"vachg", 5});
      sum += rx_exec_len(RX[38], &(Str){"hy", 2});
      sum += rx_exec_len(RX[38], &str26);
      sum += rx_exec_len(RX[191], &str26);
      sum += rx_exec_len(RX[192], &(Str){"Jva32", 5});
      sum += rx_exec_len(RX[193], &(Str){"fgngvp", 6});
    }
    for (int i = 0; i < 6; i++) {
      sum += rx_replace_len(RX[194], &(Str){"", 0});
      sum += rx_replace_len(RX[40], &(Str){"/", 1});
      sum += rx_replace_len(RX[10], &(Str){"/", 1});
      sum += rx_replace_len(RX[51], &(Str){"/", 1});
      sum += rx_replace_len(RX[52], &(Str){"/", 1});
      sum += rx_replace_len(RX[53], &(Str){"/", 1});
      sum += rx_replace_len(RX[39], &(Str){"/", 1});
      sum += rx_replace_len(RX[54], &(Str){"/", 1});
      sum += rx_replace_len(RX[63], &(Str){"uggc://zfacbegny.112.2b7.arg/o/ff/zfacbegnyubzr/1/U.7-cqi-2/{0}\?[NDO]&{1}&{2}&[NDR]", 83});
      sum += rx_replace_len(RX[12], &str41);
      sum += rx_replace_len(RX[23], &(Str){"uggc://jjj.snprobbx.pbz/fepu.cuc", 32});
      sum += rx_replace_len(RX[40], &(Str){"freivpr", 7});
      sum += rx_replace_len(RX[41], &(Str){"freivpr", 7});
      sum += rx_replace_len(RX[42], &(Str){"freivpr", 7});
      sum += rx_replace_len(RX[43], &(Str){"freivpr", 7});
      sum += rx_replace_len(RX[44], &(Str){"freivpr", 7});
      sum += rx_replace_len(RX[45], &(Str){"freivpr", 7});
      sum += rx_replace_len(RX[46], &(Str){"freivpr", 7});
      sum += rx_replace_len(RX[47], &(Str){"freivpr", 7});
      sum += rx_replace_len(RX[48], &(Str){"freivpr", 7});
      sum += rx_exec_len(RX[195], &s15[i]);
      sum += rx_exec_len(RX[66], &(Str){"", 0});
      sum += rx_exec_len(RX[50], &(Str){"fryrpgrq", 8});
      sum += rx_exec_len(RX[8], &(Str){"8sqq78r9n442851q565599o401385sp3s04r92rnn7o19ssn", 48});
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669340386893867", 35});
      sum += rx_exec_len(RX[8], &(Str){"VC=74.125.75.17", 15});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=8sqq78r9n442851q565599o401385sp3s04r92rnn7o19ssn", 60});
      sum += rx_exec_len(RX[196], &s15[i]);
      sum += rx_exec_len(RX[13], &str41);
      sum += rx_exec_len(RX[49], &(Str){"unfsbphf", 8});
    }
  return sum;
}

static int64_t runBlock9(void) {
  int64_t sum = 0;
    for (int i = 0; i < 5; i++) {
      sum += rx_split_len(RX[32], &s94[i]);
      sum += rx_split_len(RX[32], &s95[i]);
      sum += rx_split_len(RX[20], &(Str){"svz_zlfcnpr_hfre-ivrj-pbzzragf,svz_zlfcnpr_havgrq-fgngrf", 56});
      sum += rx_replace_len(RX[33], &s96[i]);
      sum += rx_replace_len(RX[67], &(Str){"zrah_arj zrah_arj_gbttyr zrah_gbttyr", 36});
      sum += rx_replace_len(RX[67], &(Str){"zrah_byq zrah_byq_gbttyr zrah_gbttyr", 36});
      sum += rx_exec_len(RX[8], &(Str){"102n9o0o9pq60132qn0337rr867p75953502q2s27s2s5r98", 48});
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231364380", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231364380.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.3931862196947939300.1231364380.1231364380.1231364380.1", 64});
      sum += rx_exec_len(RX[8], &(Str){"441326q33660", 12});
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669341278771470", 35});
      sum += rx_exec_len(RX[8], &str45);
      sum += rx_exec_len(RX[8], &str46);
      sum += rx_exec_len(RX[8], &(Str){"AFP_zp_dfctwzssrwh-aowb_80=441326q33660", 39});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=102n9o0o9pq60132qn0337rr867p75953502q2s27s2s5r98", 60});
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.3931862196947939300.1231364380.1231364380.1231364380.1", 71});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231364380", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231364380.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
    }
    for (int i = 0; i < 4; i++) {
      sum += rx_replace_len(RX[14], &(Str){" yvfg1", 6});
      sum += rx_replace_len(RX[15], &(Str){" yvfg1", 6});
      sum += rx_replace_len(RX[14], &(Str){" yvfg2", 6});
      sum += rx_replace_len(RX[15], &(Str){" yvfg2", 6});
      sum += rx_replace_len(RX[14], &(Str){" frneputebhc1", 13});
      sum += rx_replace_len(RX[15], &(Str){" frneputebhc1", 13});
      sum += rx_replace_len(RX[68], &s97[i]);
      sum += rx_replace_len(RX[18], &s97[i]);
      sum += rx_replace_len(RX[197], &(Str){"", 0});
      sum += rx_replace_len(RX[35], &(Str){"", 0});
      sum += rx_replace_len(RX[63], &(Str){"(..-{0})(|(d+)|)", 16});
      sum += rx_replace_len(RX[18], &s98[i]);
      sum += rx_replace_len(RX[56], &(Str){"//vzt.jro.qr/vij/FC/${cngu}/${anzr}/${inyhr}\?gf=${abj}", 54});
      sum += rx_replace_len(RX[198], &(Str){"//vzt.jro.qr/vij/FC/tzk_uc/${anzr}/${inyhr}\?gf=${abj}", 53});
      sum += rx_replace_len(RX[69], &(Str){"<fcna pynff=\"urnq\"><o>Jvaqbjf Yvir Ubgznvy</o></fcna><fcna pynff=\"zft\">{1}</fcna>", 81});
      sum += rx_replace_len(RX[63], &(Str){"<fcna pynff=\"urnq\"><o>{0}</o></fcna><fcna pynff=\"zft\">{1}</fcna>", 64});
      sum += rx_replace_len(RX[69], &(Str){"<fcna pynff=\"fvtahc\"><n uers=uggc://jjj.ubgznvy.pbz><o>{1}</o></n></fcna>", 73});
      sum += rx_replace_len(RX[63], &(Str){"<fcna pynff=\"fvtahc\"><n uers={0}><o>{1}</o></n></fcna>", 54});
      sum += rx_replace_len(RX[15], &(Str){"Vzntrf", 6});
      sum += rx_replace_len(RX[15], &(Str){"ZFA", 3});
      sum += rx_replace_len(RX[15], &(Str){"Zncf", 4});
      sum += rx_replace_len(RX[39], &(Str){"Zbq-Vasb-Vasb-WninFpevcgUvag", 28});
      sum += rx_replace_len(RX[15], &(Str){"Arjf", 4});
      sum += rx_split_len(RX[32], &s99[i]);
      sum += rx_split_len(RX[32], &s100[i]);
      sum += rx_replace_len(RX[15], &(Str){"Ivqrb", 5});
      sum += rx_replace_len(RX[15], &(Str){"Jro", 3});
      sum += rx_replace_len(RX[39], &(Str){"n", 1});
      sum += rx_split_len(RX[70], &(Str){"nwnkFgneg", 9});
      sum += rx_split_len(RX[70], &(Str){"nwnkFgbc", 8});
      sum += rx_replace_len(RX[14], &(Str){"ovaq", 4});
      sum += rx_replace_len(RX[15], &(Str){"ovaq", 4});
      sum += rx_replace_len(RX[63], &(Str){"oevatf lbh zber. Zber fcnpr (5TO), zber frphevgl, fgvyy serr.", 61});
      sum += rx_replace_len(RX[14], &(Str){"puvyq p1 svefg qrpx", 19});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p1 svefg qrpx", 19});
      sum += rx_replace_len(RX[14], &(Str){"puvyq p1 svefg qbhoyr2", 22});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p1 svefg qbhoyr2", 22});
      sum += rx_replace_len(RX[14], &(Str){"puvyq p2 ynfg", 13});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p2 ynfg", 13});
      sum += rx_replace_len(RX[14], &(Str){"puvyq p2", 8});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p2", 8});
      sum += rx_replace_len(RX[14], &(Str){"puvyq p3", 8});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p3", 8});
      sum += rx_replace_len(RX[14], &(Str){"puvyq p4 ynfg", 13});
      sum += rx_replace_len(RX[15], &(Str){"puvyq p4 ynfg", 13});
      sum += rx_replace_len(RX[14], &(Str){"pbclevtug", 9});
      sum += rx_replace_len(RX[15], &(Str){"pbclevtug", 9});
      sum += rx_replace_len(RX[14], &(Str){"qZFAZR_1", 8});
      sum += rx_replace_len(RX[15], &(Str){"qZFAZR_1", 8});
      sum += rx_replace_len(RX[14], &(Str){"qbhoyr2 ps", 10});
      sum += rx_replace_len(RX[15], &(Str){"qbhoyr2 ps", 10});
      sum += rx_replace_len(RX[14], &(Str){"qbhoyr2", 7});
      sum += rx_replace_len(RX[15], &(Str){"qbhoyr2", 7});
      sum += rx_replace_len(RX[14], &(Str){"uqy_arj", 7});
      sum += rx_replace_len(RX[15], &(Str){"uqy_arj", 7});
      sum += rx_replace_len(RX[30], &(Str){"uc_fubccvatobk", 14});
      sum += rx_replace_len(RX[29], &(Str){"ugzy%2Rvq", 9});
      sum += rx_replace_len(RX[30], &(Str){"ugzy%2Rvq", 9});
      sum += rx_replace_len(RX[33], &s101[i]);
      sum += rx_replace_len(RX[71], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/cebgbglcr.wf${4}${5}", 64});
      sum += rx_replace_len(RX[72], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/cebgbglcr.wf${5}", 60});
      sum += rx_replace_len(RX[73], &s102[i]);
      sum += rx_replace_len(RX[69], &(Str){"uggc://zfacbegny.112.2b7.arg/o/ff/zfacbegnyubzr/1/U.7-cqi-2/f55332979829981\?[NDO]&{1}&{2}&[NDR]", 95});
      sum += rx_replace_len(RX[14], &(Str){"vztZFSG", 7});
      sum += rx_replace_len(RX[15], &(Str){"vztZFSG", 7});
      sum += rx_replace_len(RX[14], &(Str){"zfasbbg1 ps", 11});
      sum += rx_replace_len(RX[15], &(Str){"zfasbbg1 ps", 11});
      sum += rx_replace_len(RX[14], &s103[i]);
      sum += rx_replace_len(RX[15], &s103[i]);
      sum += rx_replace_len(RX[14], &(Str){"cnerag puebzr6 fvatyr1 gno fryrpgrq ovaq", 40});
      sum += rx_replace_len(RX[15], &(Str){"cnerag puebzr6 fvatyr1 gno fryrpgrq ovaq", 40});
      sum += rx_replace_len(RX[14], &(Str){"cevznel", 7});
      sum += rx_replace_len(RX[15], &(Str){"cevznel", 7});
      sum += rx_replace_len(RX[30], &(Str){"erpgnatyr", 9});
      sum += rx_replace_len(RX[14], &(Str){"frpbaqnel", 9});
      sum += rx_replace_len(RX[15], &(Str){"frpbaqnel", 9});
      sum += rx_split_len(RX[70], &(Str){"haybnq", 6});
      sum += rx_replace_len(RX[63], &(Str){"{0}{1}1", 7});
      sum += rx_replace_len(RX[69], &(Str){"|{1}1", 5});
      sum += rx_exec_len(RX[199], &(Str){"xb-xe,ra-va,gu-gu", 17});
      sum += rx_exec_len(RX[4], &(Str){"/ZlFcnprNccf/NccPnainf,45000012", 31});
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231367708", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231367708.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.2770915348920628700.1231367708.1231367708.1231367708.1", 64});
      sum += rx_exec_len(RX[8], &(Str){"4413235p3660", 12});
      sum += rx_exec_len(RX[8], &(Str){"441327q73660", 12});
      sum += rx_exec_len(RX[8], &(Str){"9995p6rp12rrnr893334ro7nq70o7p64p69rqn844prs1473", 48});
      sum += rx_exec_len(RX[8], &(Str){"SbeprqRkcvengvba=633669350559478880", 35});
      sum += rx_exec_len(RX[8], &str54);
      sum += rx_exec_len(RX[8], &str55);
      sum += rx_exec_len(RX[8], &(Str){"AFP_zp_dfctwzs-aowb_80=441327q73660", 35});
      sum += rx_exec_len(RX[8], &(Str){"AFP_zp_kkk-aowb_80=4413235p3660", 31});
      sum += rx_exec_len(RX[8], &(Str){"FrffvbaQQS2=9995p6rp12rrnr893334ro7nq70o7p64p69rqn844prs1473", 60});
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.2770915348920628700.1231367708.1231367708.1231367708.1", 71});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231367708", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231367708.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
      sum += rx_exec_len(RX[34], &s99[i]);
      sum += rx_exec_len(RX[34], &s100[i]);
      sum += rx_exec_len(RX[200], &s15[i]);
      sum += rx_exec_len(RX[201], &str56);
      sum += rx_exec_len(RX[74], &s97[i]);
      sum += rx_exec_len(RX[28], &(Str){"svefg npgvir svefgNpgvir", 24});
      sum += rx_exec_len(RX[28], &(Str){"ynfg", 4});
      sum += rx_exec_len(RX[202], &(Str){"m:94043|yn:37.4154|yb:-122.0585|p:HF", 36});
      sum += rx_exec_len(RX[75], &str57);
      sum += rx_exec_len(RX[75], &str58);
      sum += rx_exec_len(RX[76], &str57);
      sum += rx_exec_len(RX[76], &str58);
      sum += rx_exec_len(RX[77], &str57);
      sum += rx_exec_len(RX[77], &str58);
      sum += rx_exec_len(RX[203], &str59);
      sum += rx_exec_len(RX[78], &str57);
      sum += rx_exec_len(RX[78], &str58);
      sum += rx_exec_len(RX[204], &str59);
      sum += rx_exec_len(RX[79], &str58);
      sum += rx_exec_len(RX[79], &str60);
      sum += rx_exec_len(RX[79], &str59);
      sum += rx_exec_len(RX[205], &(Str){"m:94043|yn:37.4154|yb:-122.0585|p:HF|ue:1", 41});
      sum += rx_exec_len(RX[80], &s97[i]);
      sum += rx_exec_len(RX[61], &(Str){"cebgbglcr.wf", 12});
      sum += rx_exec_len(RX[68], &s97[i]);
      sum += rx_exec_len(RX[81], &s97[i]);
      sum += rx_exec_len(RX[82], &s97[i]);
      sum += rx_exec_len(RX[206], &s21[i]);
      sum += rx_exec_len(RX[207], &s21[i]);
      sum += rx_exec_len(RX[83], &(Str){"[bowrpg tybony]", 15});
      sum += rx_exec_len(RX[62], &s97[i]);
      sum += rx_exec_len(RX[84], &str61);
      sum += rx_exec_len(RX[84], &str62);
      sum += rx_exec_len(RX[208], &str63);
    }
  return sum;
}

static int64_t runBlock10(void) {
  int64_t sum = 0;
    for (int i = 0; i < 3; i++) {
      sum += rx_replace_len(RX[39], &(Str){"%3Szxg=ra-HF", 12});
      sum += rx_replace_len(RX[40], &(Str){"-8", 2});
      sum += rx_replace_len(RX[10], &(Str){"-8", 2});
      sum += rx_replace_len(RX[51], &(Str){"-8", 2});
      sum += rx_replace_len(RX[52], &(Str){"-8", 2});
      sum += rx_replace_len(RX[53], &(Str){"-8", 2});
      sum += rx_replace_len(RX[39], &(Str){"-8", 2});
      sum += rx_replace_len(RX[54], &(Str){"-8", 2});
      sum += rx_replace_len(RX[40], &(Str){"1.5", 3});
      sum += rx_replace_len(RX[10], &(Str){"1.5", 3});
      sum += rx_replace_len(RX[51], &(Str){"1.5", 3});
      sum += rx_replace_len(RX[52], &(Str){"1.5", 3});
      sum += rx_replace_len(RX[53], &(Str){"1.5", 3});
      sum += rx_replace_len(RX[39], &(Str){"1.5", 3});
      sum += rx_replace_len(RX[54], &(Str){"1.5", 3});
      sum += rx_replace_len(RX[40], &(Str){"1024k768", 8});
      sum += rx_replace_len(RX[10], &(Str){"1024k768", 8});
      sum += rx_replace_len(RX[51], &(Str){"1024k768", 8});
      sum += rx_replace_len(RX[52], &(Str){"1024k768", 8});
      sum += rx_replace_len(RX[53], &(Str){"1024k768", 8});
      sum += rx_replace_len(RX[39], &(Str){"1024k768", 8});
      sum += rx_replace_len(RX[54], &(Str){"1024k768", 8});
      sum += rx_replace_len(RX[40], &str64);
      sum += rx_replace_len(RX[10], &str64);
      sum += rx_replace_len(RX[51], &str64);
      sum += rx_replace_len(RX[52], &str64);
      sum += rx_replace_len(RX[53], &str64);
      sum += rx_replace_len(RX[39], &str64);
      sum += rx_replace_len(RX[54], &str64);
      sum += rx_replace_len(RX[40], &(Str){"14", 2});
      sum += rx_replace_len(RX[10], &(Str){"14", 2});
      sum += rx_replace_len(RX[51], &(Str){"14", 2});
      sum += rx_replace_len(RX[52], &(Str){"14", 2});
      sum += rx_replace_len(RX[53], &(Str){"14", 2});
      sum += rx_replace_len(RX[39], &(Str){"14", 2});
      sum += rx_replace_len(RX[54], &(Str){"14", 2});
      sum += rx_replace_len(RX[40], &(Str){"24", 2});
      sum += rx_replace_len(RX[10], &(Str){"24", 2});
      sum += rx_replace_len(RX[51], &(Str){"24", 2});
      sum += rx_replace_len(RX[52], &(Str){"24", 2});
      sum += rx_replace_len(RX[53], &(Str){"24", 2});
      sum += rx_replace_len(RX[39], &(Str){"24", 2});
      sum += rx_replace_len(RX[54], &(Str){"24", 2});
      sum += rx_replace_len(RX[40], &str65);
      sum += rx_replace_len(RX[10], &str65);
      sum += rx_replace_len(RX[51], &str65);
      sum += rx_replace_len(RX[52], &str65);
      sum += rx_replace_len(RX[53], &str65);
      sum += rx_replace_len(RX[39], &str65);
      sum += rx_replace_len(RX[54], &str65);
      sum += rx_replace_len(RX[40], &str66);
      sum += rx_replace_len(RX[10], &str66);
      sum += rx_replace_len(RX[51], &str66);
      sum += rx_replace_len(RX[52], &str66);
      sum += rx_replace_len(RX[53], &str66);
      sum += rx_replace_len(RX[39], &str66);
      sum += rx_replace_len(RX[54], &str66);
      sum += rx_replace_len(RX[40], &(Str){"9.0", 3});
      sum += rx_replace_len(RX[10], &(Str){"9.0", 3});
      sum += rx_replace_len(RX[51], &(Str){"9.0", 3});
      sum += rx_replace_len(RX[52], &(Str){"9.0", 3});
      sum += rx_replace_len(RX[53], &(Str){"9.0", 3});
      sum += rx_replace_len(RX[39], &(Str){"9.0", 3});
      sum += rx_replace_len(RX[54], &(Str){"9.0", 3});
      sum += rx_replace_len(RX[40], &(Str){"994k634", 7});
      sum += rx_replace_len(RX[10], &(Str){"994k634", 7});
      sum += rx_replace_len(RX[51], &(Str){"994k634", 7});
      sum += rx_replace_len(RX[52], &(Str){"994k634", 7});
      sum += rx_replace_len(RX[53], &(Str){"994k634", 7});
      sum += rx_replace_len(RX[39], &(Str){"994k634", 7});
      sum += rx_replace_len(RX[54], &(Str){"994k634", 7});
      sum += rx_replace_len(RX[40], &(Str){"\?zxg=ra-HF", 10});
      sum += rx_replace_len(RX[10], &(Str){"\?zxg=ra-HF", 10});
      sum += rx_replace_len(RX[51], &(Str){"\?zxg=ra-HF", 10});
      sum += rx_replace_len(RX[52], &(Str){"\?zxg=ra-HF", 10});
      sum += rx_replace_len(RX[53], &(Str){"\?zxg=ra-HF", 10});
      sum += rx_replace_len(RX[54], &(Str){"\?zxg=ra-HF", 10});
      sum += rx_replace_len(RX[25], &(Str){"PAA.pbz", 7});
      sum += rx_replace_len(RX[12], &(Str){"PAA.pbz", 7});
      sum += rx_replace_len(RX[39], &(Str){"PAA.pbz", 7});
      sum += rx_replace_len(RX[25], &(Str){"Qngr & Gvzr", 11});
      sum += rx_replace_len(RX[12], &(Str){"Qngr & Gvzr", 11});
      sum += rx_replace_len(RX[39], &(Str){"Qngr & Gvzr", 11});
      sum += rx_replace_len(RX[40], &(Str){"Frnepu Zvpebfbsg.pbz", 20});
      sum += rx_replace_len(RX[54], &(Str){"Frnepu Zvpebfbsg.pbz", 20});
      sum += rx_replace_len(RX[10], &str67);
      sum += rx_replace_len(RX[51], &str67);
      sum += rx_replace_len(RX[52], &str67);
      sum += rx_replace_len(RX[53], &str67);
      sum += rx_replace_len(RX[39], &str67);
      sum += rx_split_len(RX[32], &str68);
      sum += rx_split_len(RX[32], &str69);
      sum += rx_replace_len(RX[52], &str70);
      sum += rx_replace_len(RX[53], &str70);
      sum += rx_replace_len(RX[39], &str70);
      sum += rx_replace_len(RX[40], &str71);
      sum += rx_replace_len(RX[10], &str71);
      sum += rx_replace_len(RX[51], &str71);
      sum += rx_replace_len(RX[54], &str71);
      sum += rx_replace_len(RX[25], &(Str){"Jrngure", 7});
      sum += rx_replace_len(RX[12], &(Str){"Jrngure", 7});
      sum += rx_replace_len(RX[39], &(Str){"Jrngure", 7});
      sum += rx_replace_len(RX[25], &(Str){"LbhGhor", 7});
      sum += rx_replace_len(RX[12], &(Str){"LbhGhor", 7});
      sum += rx_replace_len(RX[39], &(Str){"LbhGhor", 7});
      sum += rx_replace_len(RX[33], &str72);
      sum += rx_replace_len(RX[209], &(Str){"erzbgr_vsenzr_1", 15});
      sum += rx_replace_len(RX[40], &str73);
      sum += rx_replace_len(RX[10], &str73);
      sum += rx_replace_len(RX[51], &str73);
      sum += rx_replace_len(RX[52], &str73);
      sum += rx_replace_len(RX[53], &str73);
      sum += rx_replace_len(RX[39], &str73);
      sum += rx_replace_len(RX[54], &str73);
      sum += rx_replace_len(RX[40], &str74);
      sum += rx_replace_len(RX[10], &str74);
      sum += rx_replace_len(RX[51], &str74);
      sum += rx_replace_len(RX[52], &str74);
      sum += rx_replace_len(RX[53], &str74);
      sum += rx_replace_len(RX[39], &str74);
      sum += rx_replace_len(RX[54], &str74);
      sum += rx_replace_len(RX[210], &(Str){"lhv-h", 5});
      sum += rx_exec_len(RX[9], &(Str){"p", 1});
      sum += rx_exec_len(RX[9], &(Str){"qz p", 4});
      sum += rx_exec_len(RX[9], &(Str){"zbqynory", 8});
      sum += rx_exec_len(RX[9], &(Str){"lhv-h svefg", 11});
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231365779", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231365779.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1877536177953918500.1231365779.1231365779.1231365779.1", 64});
      sum += rx_exec_len(RX[8], &str75);
      sum += rx_exec_len(RX[8], &str76);
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.1877536177953918500.1231365779.1231365779.1231365779.1", 71});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231365779", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231365779.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
      sum += rx_exec_len(RX[34], &str68);
      sum += rx_exec_len(RX[34], &str69);
      sum += rx_exec_len(RX[211], &(Str){"", 0});
      sum += rx_exec_len(RX[31], &(Str){"qr", 2});
      sum += rx_exec_len(RX[212], &(Str){"", 0});
      sum += rx_exec_len(RX[213], &(Str){"", 0});
      sum += rx_exec_len(RX[214], &(Str){"", 0});
      sum += rx_exec_len(RX[85], &(Str){"zbqobkva zbqobk_abcnqqvat ", 26});
      sum += rx_exec_len(RX[85], &(Str){"zbqgvgyr", 8});
      sum += rx_exec_len(RX[85], &(Str){"eaq_zbqobkva ", 13});
      sum += rx_exec_len(RX[85], &(Str){"eaq_zbqgvgyr ", 13});
      sum += rx_exec_len(RX[215], &(Str){"obggbz_ani", 10});
    }
  return sum;
}

static int64_t runBlock11(void) {
  int64_t sum = 0;
    for (int i = 0; i < 2; i++) {
      sum += rx_replace_len(RX[18], &(Str){" .pybfr", 7});
      sum += rx_replace_len(RX[18], &(Str){" n.svryqOgaPnapry", 17});
      sum += rx_replace_len(RX[18], &(Str){" qg", 3});
      sum += rx_replace_len(RX[68], &str77);
      sum += rx_replace_len(RX[18], &str77);
      sum += rx_replace_len(RX[39], &(Str){"", 0});
      sum += rx_replace_len(RX[216], &(Str){"", 0});
      sum += rx_split_len(RX[86], &(Str){"", 0});
      sum += rx_replace_len(RX[39], &(Str){"*", 1});
      sum += rx_replace_len(RX[68], &(Str){"*", 1});
      sum += rx_replace_len(RX[18], &(Str){"*", 1});
      sum += rx_replace_len(RX[68], &(Str){".pybfr", 6});
      sum += rx_replace_len(RX[18], &(Str){".pybfr", 6});
      sum += rx_replace_len(RX[87], &(Str){"//vzt.jro.qr/vij/FC/tzk_uc/fperra/${inyhr}\?gf=${abj}", 52});
      sum += rx_replace_len(RX[88], &(Str){"//vzt.jro.qr/vij/FC/tzk_uc/fperra/1024\?gf=${abj}", 48});
      sum += rx_replace_len(RX[87], &(Str){"//vzt.jro.qr/vij/FC/tzk_uc/jvafvmr/${inyhr}\?gf=${abj}", 53});
      sum += rx_replace_len(RX[88], &(Str){"//vzt.jro.qr/vij/FC/tzk_uc/jvafvmr/992/608\?gf=${abj}", 52});
      sum += rx_replace_len(RX[30], &(Str){"300k120", 7});
      sum += rx_replace_len(RX[30], &(Str){"300k250", 7});
      sum += rx_replace_len(RX[30], &(Str){"310k120", 7});
      sum += rx_replace_len(RX[30], &(Str){"310k170", 7});
      sum += rx_replace_len(RX[30], &(Str){"310k250", 7});
      sum += rx_replace_len(RX[217], &(Str){"9.0  e115", 9});
      sum += rx_replace_len(RX[2], &(Str){"Nppbeqvba", 9});
      sum += rx_replace_len(RX[89], &(Str){"Nxghryy\n", 8});
      sum += rx_replace_len(RX[90], &(Str){"Nxghryy\n", 8});
      sum += rx_replace_len(RX[2], &(Str){"Nccyvpngvba", 11});
      sum += rx_replace_len(RX[89], &(Str){"Oyvpxchaxg\n", 11});
      sum += rx_replace_len(RX[90], &(Str){"Oyvpxchaxg\n", 11});
      sum += rx_replace_len(RX[89], &(Str){"Svanamra\n", 9});
      sum += rx_replace_len(RX[90], &(Str){"Svanamra\n", 9});
      sum += rx_replace_len(RX[89], &(Str){"Tnzrf\n", 6});
      sum += rx_replace_len(RX[90], &(Str){"Tnzrf\n", 6});
      sum += rx_replace_len(RX[89], &(Str){"Ubebfxbc\n", 9});
      sum += rx_replace_len(RX[90], &(Str){"Ubebfxbc\n", 9});
      sum += rx_replace_len(RX[89], &(Str){"Xvab\n", 5});
      sum += rx_replace_len(RX[90], &(Str){"Xvab\n", 5});
      sum += rx_replace_len(RX[2], &(Str){"Zbqhyrf", 7});
      sum += rx_replace_len(RX[89], &(Str){"Zhfvx\n", 6});
      sum += rx_replace_len(RX[90], &(Str){"Zhfvx\n", 6});
      sum += rx_replace_len(RX[89], &(Str){"Anpuevpugra\n", 12});
      sum += rx_replace_len(RX[90], &(Str){"Anpuevpugra\n", 12});
      sum += rx_replace_len(RX[2], &(Str){"Cuk", 3});
      sum += rx_split_len(RX[70], &(Str){"ErdhrfgSvavfu", 13});
      sum += rx_split_len(RX[70], &(Str){"ErdhrfgSvavfu.NWNK.Cuk", 22});
      sum += rx_replace_len(RX[89], &(Str){"Ebhgr\n", 6});
      sum += rx_replace_len(RX[90], &(Str){"Ebhgr\n", 6});
      sum += rx_split_len(RX[32], &str78);
      sum += rx_split_len(RX[32], &str79);
      sum += rx_split_len(RX[32], &str80);
      sum += rx_split_len(RX[32], &str81);
      sum += rx_replace_len(RX[89], &(Str){"Fcbeg\n", 6});
      sum += rx_replace_len(RX[90], &(Str){"Fcbeg\n", 6});
      sum += rx_replace_len(RX[89], &(Str){"GI-Fcbg\n", 8});
      sum += rx_replace_len(RX[90], &(Str){"GI-Fcbg\n", 8});
      sum += rx_replace_len(RX[89], &(Str){"Gbhe\n", 5});
      sum += rx_replace_len(RX[90], &(Str){"Gbhe\n", 5});
      sum += rx_replace_len(RX[89], &(Str){"Hagreunyghat\n", 13});
      sum += rx_replace_len(RX[90], &(Str){"Hagreunyghat\n", 13});
      sum += rx_replace_len(RX[89], &(Str){"Ivqrb\n", 6});
      sum += rx_replace_len(RX[90], &(Str){"Ivqrb\n", 6});
      sum += rx_replace_len(RX[89], &(Str){"Jrggre\n", 7});
      sum += rx_replace_len(RX[90], &(Str){"Jrggre\n", 7});
      sum += rx_replace_len(RX[68], &str82);
      sum += rx_replace_len(RX[18], &str82);
      sum += rx_replace_len(RX[68], &str83);
      sum += rx_replace_len(RX[18], &str83);
      sum += rx_replace_len(RX[68], &str84);
      sum += rx_replace_len(RX[18], &str84);
      sum += rx_replace_len(RX[30], &(Str){"nqiFreivprObk", 13});
      sum += rx_replace_len(RX[30], &(Str){"nqiFubccvatObk", 14});
      sum += rx_replace_len(RX[39], &(Str){"nwnk", 4});
      sum += rx_replace_len(RX[40], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[41], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[42], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[43], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[44], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[45], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[46], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[47], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[48], &(Str){"nxghryy", 7});
      sum += rx_replace_len(RX[40], &str85);
      sum += rx_replace_len(RX[41], &str85);
      sum += rx_replace_len(RX[42], &str85);
      sum += rx_replace_len(RX[43], &str85);
      sum += rx_replace_len(RX[44], &str85);
      sum += rx_replace_len(RX[45], &str85);
      sum += rx_replace_len(RX[46], &str85);
      sum += rx_replace_len(RX[47], &str85);
      sum += rx_replace_len(RX[48], &str85);
      sum += rx_replace_len(RX[29], &(Str){"pngrtbel", 8});
      sum += rx_replace_len(RX[30], &(Str){"pngrtbel", 8});
      sum += rx_replace_len(RX[39], &(Str){"pybfr", 5});
      sum += rx_replace_len(RX[39], &(Str){"qvi", 3});
      sum += rx_replace_len(RX[68], &str86);
      sum += rx_replace_len(RX[18], &str86);
      sum += rx_replace_len(RX[39], &(Str){"qg", 2});
      sum += rx_replace_len(RX[68], &(Str){"qg", 2});
      sum += rx_replace_len(RX[18], &(Str){"qg", 2});
      sum += rx_replace_len(RX[39], &(Str){"rzorq", 5});
      sum += rx_replace_len(RX[68], &(Str){"rzorq", 5});
      sum += rx_replace_len(RX[18], &(Str){"rzorq", 5});
      sum += rx_replace_len(RX[39], &(Str){"svryqOga", 8});
      sum += rx_replace_len(RX[39], &(Str){"svryqOgaPnapry", 14});
      sum += rx_split_len(RX[20], &(Str){"svz_zlfcnpr_nccf-pnainf,svz_zlfcnpr_havgrq-fgngrf", 49});
      sum += rx_replace_len(RX[40], &(Str){"svanamra", 8});
      sum += rx_replace_len(RX[41], &(Str){"svanamra", 8});
      sum += rx_replace_len(RX[42], &(Str){"svanamra", 8});
      sum += rx_replace_len(RX[43], &(Str){"svanamra", 8});
      sum += rx_replace_len(RX[44], &(Str){"svanamra", 8});
      sum += rx_replace_len(RX[45], &(Str){"svanamra", 8});
      sum += rx_replace_len(RX[46], &(Str){"svanamra", 8});
      sum += rx_replace_len(RX[47], &(Str){"svanamra", 8});
      sum += rx_replace_len(RX[48], &(Str){"svanamra", 8});
      sum += rx_split_len(RX[70], &(Str){"sbphf", 5});
      sum += rx_split_len(RX[70], &(Str){"sbphf.gno sbphfva.gno", 21});
      sum += rx_split_len(RX[70], &(Str){"sbphfva", 7});
      sum += rx_replace_len(RX[39], &(Str){"sbez", 4});
      sum += rx_replace_len(RX[68], &(Str){"sbez.nwnk", 9});
      sum += rx_replace_len(RX[18], &(Str){"sbez.nwnk", 9});
      sum += rx_replace_len(RX[40], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[41], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[42], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[43], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[44], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[45], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[46], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[47], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[48], &(Str){"tnzrf", 5});
      sum += rx_replace_len(RX[30], &(Str){"ubzrcntr", 8});
      sum += rx_replace_len(RX[40], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[41], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[42], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[43], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[44], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[45], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[46], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[47], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[48], &(Str){"ubebfxbc", 8});
      sum += rx_replace_len(RX[30], &(Str){"uc_cebzbobk_ugzy%2Puc_cebzbobk_vzt", 34});
      sum += rx_replace_len(RX[30], &(Str){"uc_erpgnatyr", 12});
      sum += rx_replace_len(RX[33], &str87);
      sum += rx_replace_len(RX[33], &str88);
      sum += rx_replace_len(RX[71], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/onfr.wf${4}${5}", 59});
      sum += rx_replace_len(RX[72], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/onfr.wf${5}", 55});
      sum += rx_replace_len(RX[71], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/qlaYvo.wf${4}${5}", 61});
      sum += rx_replace_len(RX[72], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/qlaYvo.wf${5}", 57});
      sum += rx_replace_len(RX[71], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/rssrpgYvo.wf${4}${5}", 64});
      sum += rx_replace_len(RX[72], &(Str){"uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/rssrpgYvo.wf${5}", 60});
      sum += rx_replace_len(RX[73], &str89);
      sum += rx_replace_len(RX[69], &(Str){"uggc://zfacbegny.112.2b7.arg/o/ff/zfacbegnyubzr/1/U.7-cqi-2/f55023338617756\?[NDO]&{1}&{2}&[NDR]", 95});
      sum += rx_replace_len(RX[23], &str6);
      sum += rx_replace_len(RX[40], &(Str){"xvab", 4});
      sum += rx_replace_len(RX[41], &(Str){"xvab", 4});
      sum += rx_replace_len(RX[42], &(Str){"xvab", 4});
      sum += rx_replace_len(RX[43], &(Str){"xvab", 4});
      sum += rx_replace_len(RX[44], &(Str){"xvab", 4});
      sum += rx_replace_len(RX[45], &(Str){"xvab", 4});
      sum += rx_replace_len(RX[46], &(Str){"xvab", 4});
      sum += rx_replace_len(RX[47], &(Str){"xvab", 4});
      sum += rx_replace_len(RX[48], &(Str){"xvab", 4});
      sum += rx_split_len(RX[70], &(Str){"ybnq", 4});
      sum += rx_replace_len(RX[18], &(Str){"zrqvnzbqgno lhv-anifrg lhv-anifrg-gbc", 37});
      sum += rx_replace_len(RX[39], &(Str){"zrgn", 4});
      sum += rx_replace_len(RX[68], &str90);
      sum += rx_replace_len(RX[18], &str90);
      sum += rx_split_len(RX[70], &(Str){"zbhfrzbir", 9});
      sum += rx_split_len(RX[70], &(Str){"zbhfrzbir.gno", 13});
      sum += rx_replace_len(RX[218], &str63);
      sum += rx_replace_len(RX[40], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[41], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[42], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[43], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[44], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[45], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[46], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[47], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[48], &(Str){"zhfvx", 5});
      sum += rx_replace_len(RX[52], &(Str){"zlfcnpr_nccf_pnainf", 19});
      sum += rx_replace_len(RX[40], &str91);
      sum += rx_replace_len(RX[41], &str91);
      sum += rx_replace_len(RX[42], &str91);
      sum += rx_replace_len(RX[43], &str91);
      sum += rx_replace_len(RX[44], &str91);
      sum += rx_replace_len(RX[45], &str91);
      sum += rx_replace_len(RX[46], &str91);
      sum += rx_replace_len(RX[47], &str91);
      sum += rx_replace_len(RX[48], &str91);
      sum += rx_replace_len(RX[39], &(Str){"anzr", 4});
      sum += rx_replace_len(RX[219], &str92);
      sum += rx_replace_len(RX[39], &(Str){"bow-nppbeqvba", 13});
      sum += rx_replace_len(RX[39], &(Str){"bowrpg", 6});
      sum += rx_replace_len(RX[68], &(Str){"bowrpg", 6});
      sum += rx_replace_len(RX[18], &(Str){"bowrpg", 6});
      sum += rx_replace_len(RX[29], &(Str){"cnenzf%2Rfglyrf", 15});
      sum += rx_replace_len(RX[30], &(Str){"cnenzf%2Rfglyrf", 15});
      sum += rx_replace_len(RX[30], &(Str){"cbchc", 5});
      sum += rx_replace_len(RX[40], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[41], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[42], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[43], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[44], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[45], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[46], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[47], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[48], &(Str){"ebhgr", 5});
      sum += rx_replace_len(RX[30], &(Str){"freivprobk_uc", 13});
      sum += rx_replace_len(RX[30], &(Str){"fubccvatobk_uc", 14});
      sum += rx_replace_len(RX[39], &(Str){"fubhgobk", 8});
      sum += rx_replace_len(RX[40], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[41], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[42], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[43], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[44], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[45], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[46], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[47], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[48], &(Str){"fcbeg", 5});
      sum += rx_replace_len(RX[40], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[41], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[42], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[43], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[44], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[45], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[46], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[47], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[48], &(Str){"gbhe", 4});
      sum += rx_replace_len(RX[40], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[41], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[42], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[43], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[44], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[45], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[46], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[47], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[48], &(Str){"gi-fcbg", 7});
      sum += rx_replace_len(RX[39], &(Str){"glcr", 4});
      sum += rx_replace_len(RX[220], &(Str){"haqrsvarq", 9});
      sum += rx_replace_len(RX[40], &str93);
      sum += rx_replace_len(RX[41], &str93);
      sum += rx_replace_len(RX[42], &str93);
      sum += rx_replace_len(RX[43], &str93);
      sum += rx_replace_len(RX[44], &str93);
      sum += rx_replace_len(RX[45], &str93);
      sum += rx_replace_len(RX[46], &str93);
      sum += rx_replace_len(RX[47], &str93);
      sum += rx_replace_len(RX[48], &str93);
      sum += rx_replace_len(RX[40], &(Str){"ivqrb", 5});
      sum += rx_replace_len(RX[41], &(Str){"ivqrb", 5});
      sum += rx_replace_len(RX[42], &(Str){"ivqrb", 5});
      sum += rx_replace_len(RX[43], &(Str){"ivqrb", 5});
      sum += rx_replace_len(RX[44], &(Str){"ivqrb", 5});
      sum += rx_replace_len(RX[45], &(Str){"ivqrb", 5});
      sum += rx_replace_len(RX[46], &(Str){"ivqrb", 5});
      sum += rx_replace_len(RX[47], &(Str){"ivqrb", 5});
      sum += rx_replace_len(RX[48], &(Str){"ivqrb", 5});
      sum += rx_split_len(RX[86], &(Str){"ivfvgf=1", 8});
      sum += rx_replace_len(RX[40], &(Str){"jrggre", 6});
      sum += rx_replace_len(RX[41], &(Str){"jrggre", 6});
      sum += rx_replace_len(RX[42], &(Str){"jrggre", 6});
      sum += rx_replace_len(RX[43], &(Str){"jrggre", 6});
      sum += rx_replace_len(RX[44], &(Str){"jrggre", 6});
      sum += rx_replace_len(RX[45], &(Str){"jrggre", 6});
      sum += rx_replace_len(RX[46], &(Str){"jrggre", 6});
      sum += rx_replace_len(RX[47], &(Str){"jrggre", 6});
      sum += rx_replace_len(RX[48], &(Str){"jrggre", 6});
      sum += rx_exec_len(RX[221], &(Str){"uggc://jjj.fpuhryreim.arg/Qrsnhyg", 33});
      sum += rx_exec_len(RX[66], &(Str){"fryrpgrq", 8});
      sum += rx_exec_len(RX[222], &(Str){"sff lhv-ani", 11});
      sum += rx_exec_len(RX[223], &(Str){"zrqvnzbqgno lhv-anifrg", 22});
      sum += rx_exec_len(RX[224], &(Str){"zrqvnzbqgno lhv-anifrg", 22});
      sum += rx_exec_len(RX[91], &(Str){"GnoThvq", 7});
      sum += rx_exec_len(RX[91], &(Str){"thvq", 4});
      sum += rx_exec_len(RX[225], &str63);
      sum += rx_exec_len(RX[226], &str63);
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231365869", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.0.10.1231367054", 25});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231365869.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1231367054.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 70});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1670816052019209000.1231365869.1231365869.1231365869.1", 64});
      sum += rx_exec_len(RX[8], &(Str){"144631658.1796080716621419500.1231367054.1231367054.1231367054.1", 64});
      sum += rx_exec_len(RX[8], &str94);
      sum += rx_exec_len(RX[8], &str95);
      sum += rx_exec_len(RX[8], &str96);
      sum += rx_exec_len(RX[8], &str97);
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.1670816052019209000.1231365869.1231365869.1231365869.1", 71});
      sum += rx_exec_len(RX[8], &(Str){"__hgzn=144631658.1796080716621419500.1231367054.1231367054.1231367054.1", 71});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231365869", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzo=144631658.0.10.1231367054", 32});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231365869.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
      sum += rx_exec_len(RX[8], &(Str){"__hgzm=144631658.1231367054.1.1.hgzpfe=(qverpg)|hgzppa=(qverpg)|hgzpzq=(abar)", 77});
      sum += rx_exec_len(RX[34], &str78);
      sum += rx_exec_len(RX[34], &str79);
      sum += rx_exec_len(RX[34], &str81);
      sum += rx_exec_len(RX[74], &str77);
      sum += rx_exec_len(RX[74], &(Str){"*", 1});
      sum += rx_exec_len(RX[74], &str82);
      sum += rx_exec_len(RX[74], &str83);
      sum += rx_exec_len(RX[74], &str86);
      sum += rx_exec_len(RX[74], &(Str){"rzorq", 5});
      sum += rx_exec_len(RX[74], &(Str){"sbez.nwnk", 9});
      sum += rx_exec_len(RX[74], &str90);
      sum += rx_exec_len(RX[74], &(Str){"bowrpg", 6});
      sum += rx_exec_len(RX[227], &(Str){"/uggc://wf.hv-cbegny.qr/tzk/ubzr/wf/20080602/onfr.wf", 52});
      sum += rx_exec_len(RX[28], &(Str){"uvag ynfgUvag ynfg", 18});
      sum += rx_exec_len(RX[75], &(Str){"", 0});
      sum += rx_exec_len(RX[76], &(Str){"", 0});
      sum += rx_exec_len(RX[77], &(Str){"", 0});
      sum += rx_exec_len(RX[78], &(Str){"", 0});
      sum += rx_exec_len(RX[80], &str77);
      sum += rx_exec_len(RX[80], &(Str){"*", 1});
      sum += rx_exec_len(RX[80], &(Str){".pybfr", 6});
      sum += rx_exec_len(RX[80], &str82);
      sum += rx_exec_len(RX[80], &str83);
      sum += rx_exec_len(RX[80], &str84);
      sum += rx_exec_len(RX[80], &str86);
      sum += rx_exec_len(RX[80], &(Str){"qg", 2});
      sum += rx_exec_len(RX[80], &(Str){"rzorq", 5});
      sum += rx_exec_len(RX[80], &(Str){"sbez.nwnk", 9});
      sum += rx_exec_len(RX[80], &str90);
      sum += rx_exec_len(RX[80], &(Str){"bowrpg", 6});
      sum += rx_exec_len(RX[61], &(Str){"qlaYvo.wf", 9});
      sum += rx_exec_len(RX[61], &(Str){"rssrpgYvo.wf", 12});
      sum += rx_exec_len(RX[61], &(Str){"uggc://jjj.tzk.arg/qr/\?fgnghf=uvajrvf", 37});
      sum += rx_exec_len(RX[92], &(Str){" .pybfr", 7});
      sum += rx_exec_len(RX[92], &(Str){" n.svryqOgaPnapry", 17});
      sum += rx_exec_len(RX[92], &(Str){" qg", 3});
      sum += rx_exec_len(RX[92], &str48);
      sum += rx_exec_len(RX[92], &(Str){".nwnk", 5});
      sum += rx_exec_len(RX[92], &(Str){".svryqOga,n.svryqOgaPnapry", 26});
      sum += rx_exec_len(RX[92], &(Str){".svryqOgaPnapry", 15});
      sum += rx_exec_len(RX[92], &(Str){".bow-nppbeqvba qg", 17});
      sum += rx_exec_len(RX[68], &str77);
      sum += rx_exec_len(RX[68], &(Str){"*", 1});
      sum += rx_exec_len(RX[68], &(Str){".pybfr", 6});
      sum += rx_exec_len(RX[68], &str82);
      sum += rx_exec_len(RX[68], &str83);
      sum += rx_exec_len(RX[68], &str84);
      sum += rx_exec_len(RX[68], &str86);
      sum += rx_exec_len(RX[68], &(Str){"qg", 2});
      sum += rx_exec_len(RX[68], &(Str){"rzorq", 5});
      sum += rx_exec_len(RX[68], &(Str){"sbez.nwnk", 9});
      sum += rx_exec_len(RX[68], &str90);
      sum += rx_exec_len(RX[68], &(Str){"bowrpg", 6});
      sum += rx_exec_len(RX[93], &(Str){" .pybfr", 7});
      sum += rx_exec_len(RX[93], &(Str){" n.svryqOgaPnapry", 17});
      sum += rx_exec_len(RX[93], &(Str){" qg", 3});
      sum += rx_exec_len(RX[93], &str48);
      sum += rx_exec_len(RX[93], &(Str){".nwnk", 5});
      sum += rx_exec_len(RX[93], &(Str){".svryqOga,n.svryqOgaPnapry", 26});
      sum += rx_exec_len(RX[93], &(Str){".svryqOgaPnapry", 15});
      sum += rx_exec_len(RX[93], &(Str){".bow-nppbeqvba qg", 17});
      sum += rx_exec_len(RX[81], &str77);
      sum += rx_exec_len(RX[81], &(Str){"*", 1});
      sum += rx_exec_len(RX[81], &str48);
      sum += rx_exec_len(RX[81], &(Str){".pybfr", 6});
      sum += rx_exec_len(RX[81], &str82);
      sum += rx_exec_len(RX[81], &str83);
      sum += rx_exec_len(RX[81], &str84);
      sum += rx_exec_len(RX[81], &str86);
      sum += rx_exec_len(RX[81], &(Str){"qg", 2});
      sum += rx_exec_len(RX[81], &(Str){"rzorq", 5});
      sum += rx_exec_len(RX[81], &(Str){"sbez.nwnk", 9});
      sum += rx_exec_len(RX[81], &str90);
      sum += rx_exec_len(RX[81], &(Str){"bowrpg", 6});
      sum += rx_exec_len(RX[94], &(Str){" .pybfr", 7});
      sum += rx_exec_len(RX[94], &(Str){" n.svryqOgaPnapry", 17});
      sum += rx_exec_len(RX[94], &(Str){" qg", 3});
      sum += rx_exec_len(RX[94], &str48);
      sum += rx_exec_len(RX[94], &(Str){".nwnk", 5});
      sum += rx_exec_len(RX[94], &(Str){".svryqOga,n.svryqOgaPnapry", 26});
      sum += rx_exec_len(RX[94], &(Str){".svryqOgaPnapry", 15});
      sum += rx_exec_len(RX[94], &(Str){".bow-nppbeqvba qg", 17});
      sum += rx_exec_len(RX[94], &(Str){"[anzr=nwnkHey]", 14});
      sum += rx_exec_len(RX[94], &str82);
      sum += rx_exec_len(RX[31], &(Str){"rf", 2});
      sum += rx_exec_len(RX[31], &(Str){"wn", 2});
      sum += rx_exec_len(RX[82], &str77);
      sum += rx_exec_len(RX[82], &(Str){"*", 1});
      sum += rx_exec_len(RX[82], &str48);
      sum += rx_exec_len(RX[82], &(Str){".pybfr", 6});
      sum += rx_exec_len(RX[82], &str82);
      sum += rx_exec_len(RX[82], &str83);
      sum += rx_exec_len(RX[82], &str84);
      sum += rx_exec_len(RX[82], &str86);
      sum += rx_exec_len(RX[82], &(Str){"qg", 2});
      sum += rx_exec_len(RX[82], &(Str){"rzorq", 5});
      sum += rx_exec_len(RX[82], &(Str){"sbez.nwnk", 9});
      sum += rx_exec_len(RX[82], &str90);
      sum += rx_exec_len(RX[82], &(Str){"bowrpg", 6});
      sum += rx_exec_len(RX[83], &str98);
      sum += rx_exec_len(RX[83], &(Str){"shapgvba sbphf() { [angvir pbqr] }", 34});
      sum += rx_exec_len(RX[62], &(Str){"#Ybtva", 6});
      sum += rx_exec_len(RX[62], &(Str){"#Ybtva_cnffjbeq", 15});
      sum += rx_exec_len(RX[62], &str77);
      sum += rx_exec_len(RX[62], &(Str){"#fubhgobkWf", 11});
      sum += rx_exec_len(RX[62], &(Str){"#fubhgobkWfReebe", 16});
      sum += rx_exec_len(RX[62], &(Str){"#fubhgobkWfFhpprff", 18});
      sum += rx_exec_len(RX[62], &(Str){"*", 1});
      sum += rx_exec_len(RX[62], &str82);
      sum += rx_exec_len(RX[62], &str83);
      sum += rx_exec_len(RX[62], &str86);
      sum += rx_exec_len(RX[62], &(Str){"rzorq", 5});
      sum += rx_exec_len(RX[62], &(Str){"sbez.nwnk", 9});
      sum += rx_exec_len(RX[62], &str90);
      sum += rx_exec_len(RX[62], &(Str){"bowrpg", 6});
      sum += rx_exec_len(RX[49], &(Str){"pbagrag", 7});
      sum += rx_exec_len(RX[24], &str6);
      sum += rx_exec_len(RX[228], &str63);
      sum += rx_exec_len(RX[229], &(Str){"jva32", 5});
      sum += rx_exec_len(RX[230], &str63);
      sum += rx_exec_len(RX[231], &str63);
      sum += rx_exec_len(RX[232], &str63);
      sum += rx_exec_len(RX[233], &str63);
      sum += rx_exec_len(RX[234], &str63);
      sum += rx_exec_len(RX[235], &(Str){"jva32", 5});
      sum += rx_exec_len(RX[236], &str63);
    }
  return sum;
}

static int64_t run_regexp_benchmark(void) {
  int64_t sum = 0;
  for (int i = 0; i < 5; i++) {
    sum = 0;
    sum += runBlock0();
    sum += runBlock1();
    sum += runBlock2();
    sum += runBlock3();
    sum += runBlock4();
    sum += runBlock5();
    sum += runBlock6();
    sum += runBlock7();
    sum += runBlock8();
    sum += runBlock9();
    sum += runBlock10();
    sum += runBlock11();
    if (sum != 1666156) {
      fprintf(stderr, "regexp-octane: wrong checksum %lld\n", (long long)sum);
      failures++;
    }
  }
  return sum;
}

/* ---- Driver ---- */

int main(int argc, char **argv) {
  int64_t checksum = 0;
  int iterations = argc > 1 ? atoi(argv[1]) : ITERATIONS;
  setup_regexps();
  setup_vars();
  double t0 = now_ms();
  for (int it = 0; it < iterations; it++) checksum = run_regexp_benchmark();
  double ms = now_ms() - t0;
  printf("regexp-octane %d iterations %.3f ms checksum %lld %s\n", iterations, ms,
         (long long)checksum, failures ? "FAILED" : "ok");
  return failures ? 1 : 0;
}
