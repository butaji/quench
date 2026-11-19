// jetstream-row: FlightPlanner
// jetstream-files: RexBench/FlightPlanner/airways.js RexBench/FlightPlanner/waypoints.js.z RexBench/FlightPlanner/flight_planner.js RexBench/FlightPlanner/expectations.js RexBench/FlightPlanner/benchmark.js
// iterations: 120
/* C analogue of JetStream's RexBench/FlightPlanner: the same rally/airway route parser, the same
 * great-circle, wind and turn arithmetic, the same user and FAA waypoint tables, and the same
 * validation against expectations.js. The timed loop is the JS runIteration: five times, reset
 * the user waypoints and re-parse every flight plan's route. Loading (inflating waypoints.js.z,
 * reading the airway and waypoint literals, and building the expected plans) happens before the
 * timer, as the JS shell loads its files before the benchmark runs. */
#include <ctype.h>
#include <math.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define EARTH_RADIUS 3440.0
#define PI_JS 3.141592653589793 /* Math.PI */
#define TWO_PI_JS (PI_JS * 2)

static void die(const char *msg) {
  fprintf(stderr, "FATAL: %s\n", msg);
  exit(2);
}
/* console.error / status (console.debug) */
static void js_error(const char *msg) { fprintf(stderr, "Error: %s\n", msg); }
static void status(const char *msg) { (void)msg; }

static double to_rad(double d) { return d * PI_JS / 180.0; }
static double to_deg(double r) { return r * 180.0 / PI_JS; }
static double js_round(double x) { return floor(x + 0.5); }

/* ---------- string-keyed hash table (JS object property lookup) ---------- */
typedef struct { const char *key; void *val; } Slot;
typedef struct { Slot *slot; size_t cap, n; } Table;

static unsigned long str_hash(const char *s) {
  unsigned long h = 1469598103934665603UL;
  while (*s) { h ^= (unsigned char)*s++; h *= 1099511628211UL; }
  return h;
}
static void table_init(Table *t, size_t cap) {
  t->cap = cap; t->n = 0; t->slot = calloc(cap, sizeof(Slot));
}
static void table_clear(Table *t) {
  memset(t->slot, 0, t->cap * sizeof(Slot));
  t->n = 0;
}
static void *table_get(const Table *t, const char *key) {
  size_t i = str_hash(key) & (t->cap - 1);
  while (t->slot[i].key) {
    if (!strcmp(t->slot[i].key, key)) return t->slot[i].val;
    i = (i + 1) & (t->cap - 1);
  }
  return NULL;
}
static void table_put(Table *t, const char *key, void *val) {
  if (2 * (t->n + 1) > t->cap) {
    Table bigger;
    table_init(&bigger, t->cap * 2);
    for (size_t i = 0; i < t->cap; i++)
      if (t->slot[i].key) table_put(&bigger, t->slot[i].key, t->slot[i].val);
    free(t->slot);
    *t = bigger;
  }
  size_t i = str_hash(key) & (t->cap - 1);
  while (t->slot[i].key) {
    if (!strcmp(t->slot[i].key, key)) { t->slot[i].val = val; return; }
    i = (i + 1) & (t->cap - 1);
  }
  t->slot[i].key = key;
  t->slot[i].val = val;
  t->n++;
}

/* ---------- RFC 1950/1951 inflate (zlib), puff-style canonical Huffman decoding ---------- */
typedef struct {
  const unsigned char *in; size_t inlen, pos;
  unsigned long bitbuf; int bitcnt;
  unsigned char *out; size_t outn, outcap;
} Inflate;
typedef struct { short count[16]; short symbol[320]; } Huff;

static void emit(Inflate *s, unsigned char c) {
  if (s->outn == s->outcap) {
    s->outcap = s->outcap ? s->outcap * 2 : (1 << 20);
    s->out = realloc(s->out, s->outcap);
    if (!s->out) die("out of memory inflating waypoints");
  }
  s->out[s->outn++] = c;
}
static unsigned getbits(Inflate *s, int need) {
  unsigned long v = s->bitbuf;
  while (s->bitcnt < need) {
    if (s->pos >= s->inlen) die("truncated deflate stream");
    v |= (unsigned long)s->in[s->pos++] << s->bitcnt;
    s->bitcnt += 8;
  }
  s->bitbuf = v >> need;
  s->bitcnt -= need;
  return (unsigned)(v & ((1UL << need) - 1));
}
static void huff_build(Huff *h, const short *len, int n) {
  int offs[16];
  memset(h->count, 0, sizeof h->count);
  for (int sym = 0; sym < n; sym++) h->count[len[sym]]++;
  offs[1] = 0;
  for (int l = 1; l < 15; l++) offs[l + 1] = offs[l] + h->count[l];
  for (int sym = 0; sym < n; sym++)
    if (len[sym]) h->symbol[offs[len[sym]]++] = (short)sym;
}
static int huff_decode(Inflate *s, const Huff *h) {
  int code = 0, first = 0, index = 0;
  for (int len = 1; len <= 15; len++) {
    code |= (int)getbits(s, 1);
    int count = h->count[len];
    if (code - count < first) return h->symbol[index + (code - first)];
    index += count;
    first += count;
    first <<= 1;
    code <<= 1;
  }
  die("invalid Huffman code");
  return -1;
}
static void inflate_codes(Inflate *s, const Huff *lc, const Huff *dc) {
  static const short LBASE[29] = {3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31,
                                  35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258};
  static const short LEXT[29] = {0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2,
                                 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0};
  static const short DBASE[30] = {1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129,
                                  193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097,
                                  6145, 8193, 12289, 16385, 24577};
  static const short DEXT[30] = {0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6,
                                 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13};
  for (;;) {
    int sym = huff_decode(s, lc);
    if (sym < 256) {
      emit(s, (unsigned char)sym);
    } else if (sym == 256) {
      return;
    } else {
      sym -= 257;
      if (sym >= 29) die("bad length symbol");
      size_t len = LBASE[sym] + getbits(s, LEXT[sym]);
      sym = huff_decode(s, dc);
      if (sym >= 30) die("bad distance symbol");
      size_t dist = DBASE[sym] + getbits(s, DEXT[sym]);
      if (dist > s->outn) die("distance too far back");
      for (size_t i = 0; i < len; i++) emit(s, s->out[s->outn - dist]);
    }
  }
}
static unsigned char *zlib_inflate(const unsigned char *in, size_t n, size_t *outn) {
  if (n < 6 || (in[0] & 0x0f) != 8 || ((in[0] << 8) | in[1]) % 31 != 0 || (in[1] & 0x20))
    die("not a zlib stream");
  Inflate s = {in, n, 2, 0, 0, NULL, 0, 0};
  int last;
  do {
    last = (int)getbits(&s, 1);
    unsigned type = getbits(&s, 2);
    if (type == 0) {
      s.bitbuf = 0; s.bitcnt = 0; /* discard to byte boundary */
      if (s.pos + 4 > s.inlen) die("truncated stored block");
      size_t len = s.in[s.pos] | (s.in[s.pos + 1] << 8);
      s.pos += 4;
      if (s.pos + len > s.inlen) die("truncated stored block");
      for (size_t i = 0; i < len; i++) emit(&s, s.in[s.pos++]);
    } else if (type == 1) {
      static Huff fl, fd;
      static int built = 0;
      if (!built) {
        short lens[320];
        int i = 0;
        for (; i < 144; i++) lens[i] = 8;
        for (; i < 256; i++) lens[i] = 9;
        for (; i < 280; i++) lens[i] = 7;
        for (; i < 288; i++) lens[i] = 8;
        huff_build(&fl, lens, 288);
        for (i = 0; i < 30; i++) lens[i] = 5;
        huff_build(&fd, lens, 30);
        built = 1;
      }
      inflate_codes(&s, &fl, &fd);
    } else if (type == 2) {
      static const short order[19] = {16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15};
      short lengths[320];
      short cl[19];
      Huff lc, dc, cc;
      int nlen = (int)getbits(&s, 5) + 257, ndist = (int)getbits(&s, 5) + 1, ncode = (int)getbits(&s, 4) + 4;
      memset(cl, 0, sizeof cl);
      for (int i = 0; i < ncode; i++) cl[order[i]] = (short)getbits(&s, 3);
      huff_build(&cc, cl, 19);
      memset(lengths, 0, sizeof lengths);
      int idx = 0;
      while (idx < nlen + ndist) {
        int sym = huff_decode(&s, &cc);
        if (sym < 16) {
          lengths[idx++] = (short)sym;
        } else {
          short len = 0;
          int rep;
          if (sym == 16) {
            if (idx == 0) die("repeat with no previous length");
            len = lengths[idx - 1];
            rep = 3 + (int)getbits(&s, 2);
          } else if (sym == 17) {
            rep = 3 + (int)getbits(&s, 3);
          } else {
            rep = 11 + (int)getbits(&s, 7);
          }
          while (rep--) lengths[idx++] = len;
        }
      }
      huff_build(&lc, lengths, nlen);
      huff_build(&dc, lengths + nlen, ndist);
      inflate_codes(&s, &lc, &dc);
    } else {
      die("invalid deflate block type");
    }
  } while (!last);
  *outn = s.outn;
  return s.out;
}

static unsigned char *read_file(const char *path, size_t *len) {
  FILE *f = fopen(path, "rb");
  if (!f) { fprintf(stderr, "cannot open %s\n", path); exit(2); }
  size_t cap = 1 << 22, n = 0;
  unsigned char *buf = malloc(cap + 1);
  size_t r;
  while ((r = fread(buf + n, 1, cap - n, f)) > 0) {
    n += r;
    if (n == cap) { cap *= 2; buf = realloc(buf, cap + 1); }
  }
  fclose(f);
  buf[n] = 0;
  *len = n;
  return buf;
}

/* ---------- JS literal scanning for the data files ---------- */
typedef struct { const char *p, *end; } Scan;

static void ws(Scan *s) { while (s->p < s->end && isspace((unsigned char)*s->p)) s->p++; }
static bool peek_is(Scan *s, char c) { ws(s); return s->p < s->end && *s->p == c; }
static void expect(Scan *s, char c) {
  ws(s);
  if (s->p >= s->end || *s->p != c) {
    fprintf(stderr, "expected '%c' near: %.40s\n", c, s->p);
    die("malformed JS data");
  }
  s->p++;
}
static int hexval(char c) {
  return isdigit((unsigned char)c) ? c - '0' : (tolower((unsigned char)c) - 'a' + 10);
}
/* Reads a double-quoted string (JS escapes included) into a new buffer. */
static char *scan_string(Scan *s) {
  ws(s);
  if (s->p >= s->end || *s->p != '"') die("expected string literal");
  s->p++;
  size_t cap = 32, n = 0;
  char *out = malloc(cap);
  while (s->p < s->end && *s->p != '"') {
    char c = *s->p++;
    if (c == '\\' && s->p < s->end) {
      char e = *s->p++;
      switch (e) {
      case 'n': c = '\n'; break;
      case 't': c = '\t'; break;
      case 'r': c = '\r'; break;
      case 'b': c = '\b'; break;
      case 'f': c = '\f'; break;
      case 'u': {
        unsigned v = 0;
        for (int k = 0; k < 4 && s->p < s->end; k++) v = v * 16 + (unsigned)hexval(*s->p++);
        c = v < 128 ? (char)v : '?';
        break;
      }
      default: c = e;
      }
    }
    if (n + 2 >= cap) { cap *= 2; out = realloc(out, cap); }
    out[n++] = c;
  }
  if (s->p >= s->end) die("unterminated string literal");
  s->p++;
  out[n] = 0;
  return out;
}
static double scan_number(Scan *s) {
  char *end;
  ws(s);
  double v = strtod(s->p, &end);
  if (end == s->p || end > s->end) die("expected number literal");
  s->p = end;
  return v;
}
static void skip_value(Scan *s) {
  ws(s);
  if (*s->p == '"') { free(scan_string(s)); return; }
  if (*s->p == '{' || *s->p == '[') {
    int depth = 0;
    do {
      if (*s->p == '"') { free(scan_string(s)); continue; }
      if (*s->p == '{' || *s->p == '[') depth++;
      else if (*s->p == '}' || *s->p == ']') depth--;
      s->p++;
    } while (depth > 0 && s->p < s->end);
    return;
  }
  while (s->p < s->end && !strchr(",}] \t\r\n", *s->p)) s->p++;
}

/* ---------- data: waypoints and airways ---------- */
typedef struct { const char *name; double lat, lon; } Waypoint;
typedef struct { int nfix; const char **fix; } Airway;

static Table faa_waypoints, faa_airways, user_waypoints;

static Scan open_literal(const char *text, size_t len, const char *var) {
  Scan s = {text, text + len};
  const char *k = strstr(text, var);
  if (!k) { fprintf(stderr, "missing %s\n", var); die("bad data file"); }
  s.p = k + strlen(var);
  return s;
}
static void load_waypoints(const char *text, size_t len) {
  Scan s = open_literal(text, len, "_faaWaypoints =");
  expect(&s, '{');
  if (!peek_is(&s, '}')) for (;;) {
    char *key = scan_string(&s);
    expect(&s, ':');
    expect(&s, '{');
    Waypoint *w = calloc(1, sizeof *w);
    for (;;) {
      char *f = scan_string(&s);
      expect(&s, ':');
      if (!strcmp(f, "name")) w->name = scan_string(&s);
      else if (!strcmp(f, "latitude")) w->lat = scan_number(&s);
      else if (!strcmp(f, "longitude")) w->lon = scan_number(&s);
      else skip_value(&s);
      free(f);
      if (peek_is(&s, ',')) { s.p++; continue; }
      break;
    }
    expect(&s, '}');
    if (!w->name) die("waypoint without name");
    table_put(&faa_waypoints, key, w);
    if (peek_is(&s, ',')) { s.p++; continue; }
    break;
  }
  expect(&s, '}');
}
static void load_airways(const char *text, size_t len) {
  Scan s = open_literal(text, len, "_faaAirways =");
  expect(&s, '{');
  if (!peek_is(&s, '}')) for (;;) {
    char *key = scan_string(&s);
    expect(&s, ':');
    expect(&s, '{');
    Airway *a = calloc(1, sizeof *a);
    if (!peek_is(&s, '}')) for (;;) {
      char *f = scan_string(&s);
      expect(&s, ':');
      if (!strcmp(f, "fixes")) {
        int cap = 16;
        a->fix = malloc(cap * sizeof *a->fix);
        expect(&s, '[');
        if (!peek_is(&s, ']')) for (;;) {
          if (a->nfix == cap) { cap *= 2; a->fix = realloc(a->fix, cap * sizeof *a->fix); }
          a->fix[a->nfix++] = scan_string(&s);
          if (peek_is(&s, ',')) { s.p++; continue; }
          break;
        }
        expect(&s, ']');
      } else {
        skip_value(&s);
      }
      free(f);
      if (peek_is(&s, ',')) { s.p++; continue; }
      break;
    }
    expect(&s, '}');
    table_put(&faa_airways, key, a);
    if (peek_is(&s, ',')) { s.p++; continue; }
    break;
  }
  expect(&s, '}');
}

/* zlib-compressed waypoints: inflate, then parse (both outside the timed loop). */
static void load_waypoints_z(const char *path) {
  size_t zlen;
  unsigned char *z = read_file(path, &zlen);
  size_t n;
  unsigned char *js = zlib_inflate(z, zlen, &n);
  free(z);
  js = realloc(js, n + 1);
  js[n] = 0;
  load_waypoints((const char *)js, n);
  free(js);
}

/* ---------- user waypoints (setupUserWaypoints) ---------- */
static Waypoint user_store[64];
static char user_key[64][64];
static int user_count;

/* Mirrors LatRE / LongRE (decimalLatitudeFromString / decimalLongitudeFromString) for the
 * grammar used by the workload: sign, degrees, then an optional minutes-with-decimal,
 * decimal-degrees or degrees-minutes-seconds tail. Returns 0 when the regex would not match. */
static double dec_angle(const char *s, bool isLat) {
  const char *p = s;
  double sign = 1;
  char c = *p;
  if (isLat ? (c == 'N' || c == 'n' || c == 'S' || c == 's' || c == '-')
            : (c == 'E' || c == 'e' || c == 'W' || c == 'w' || c == '-')) {
    if (c == '-' || toupper((unsigned char)c) == (isLat ? 'S' : 'W')) sign = -1;
    p++;
  }
  double result;
  if (p[0] == '9' && p[1] == '0' && isLat) {
    result = 90; p += 2;
  } else if (!isLat && p[0] == '1' && p[1] == '8' && p[2] == '0') {
    result = 180; p += 3;
  } else {
    size_t L = 0;
    if (isLat) {
      if (p[0] >= '0' && p[0] <= '8' && isdigit((unsigned char)p[1])) L = 2;
      else if (isdigit((unsigned char)p[0])) L = 1;
    } else {
      if (p[0] == '1' && p[1] >= '0' && p[1] <= '7' && isdigit((unsigned char)p[2])) L = 3;
      else if (isdigit((unsigned char)p[0]) && isdigit((unsigned char)p[1])) L = 2;
      else if (isdigit((unsigned char)p[0])) L = 1;
    }
    if (!L) return 0;
    char num[8] = {0};
    memcpy(num, p, L);
    result = strtod(num, NULL);
    p += L;
  }
  double limit = isLat ? 90 : 180;
  bool haveTail = false;
  double tail = 0;
  /* group 3: ' ' [0-5]?\d '.' \d{0,3} */
  if (p[0] == ' ') {
    const char *q = p + 1;
    for (int L = 2; L >= 1 && !haveTail; L--) {
      if (L == 2 && !(q[0] >= '0' && q[0] <= '5' && isdigit((unsigned char)q[1]))) continue;
      if (L == 1 && !isdigit((unsigned char)q[0])) continue;
      if (q[L] != '.') continue;
      const char *r = q + L + 1;
      size_t k = 0;
      while (k < 3 && isdigit((unsigned char)r[k])) k++;
      char text[24];
      snprintf(text, sizeof text, " %.*s.%.*s", L, q, (int)k, r);
      tail = strtod(text, NULL) / 60;
      p = r + k;
      haveTail = true;
    }
    /* group 5: ' ' ([0-5]?\d) '"' ' '? ([0-5]?\d) '\''? */
    for (int L = 2; L >= 1 && !haveTail; L--) {
      if (L == 2 && !(q[0] >= '0' && q[0] <= '5' && isdigit((unsigned char)q[1]))) continue;
      if (L == 1 && !isdigit((unsigned char)q[0])) continue;
      if (q[L] != '"') continue;
      const char *r = q + L + 1;
      if (*r == ' ') r++;
      for (int M = 2; M >= 1; M--) {
        if (M == 2 && !(r[0] >= '0' && r[0] <= '5' && isdigit((unsigned char)r[1]))) continue;
        if (M == 1 && !isdigit((unsigned char)r[0])) continue;
        char a[4] = {0}, b[4] = {0};
        memcpy(a, q, L);
        memcpy(b, r, M);
        double minutes = strtod(a, NULL);
        double seconds = strtod(b, NULL);
        tail = (minutes + seconds / 60) / 60;
        p = r + M;
        if (*p == '\'') p++;
        haveTail = true;
        break;
      }
    }
  } else if (p[0] == '.') {
    /* group 4: \.\d{0,6} */
    size_t k = 1;
    while (k < 7 && isdigit((unsigned char)p[k])) k++;
    char text[12];
    snprintf(text, sizeof text, ".%.*s", (int)(k - 1), p + 1);
    tail = (k == 1) ? NAN : strtod(text, NULL);
    p += k;
    haveTail = true;
  }
  if (haveTail) {
    if (result != limit) result = result + tail;
  }
  return result * sign;
}

static void user_update(const char *name, const char *lat, const char *lon) {
  double la = dec_angle(lat, true);
  double lo = dec_angle(lon, false);
  char key[64];
  size_t n = 0;
  for (; name[n] && n < 63; n++) key[n] = (char)toupper((unsigned char)name[n]);
  key[n] = 0;
  Waypoint *w = table_get(&user_waypoints, key);
  if (!w) {
    int idx = user_count++;
    strcpy(user_key[idx], key);
    w = &user_store[idx];
    table_put(&user_waypoints, user_key[idx], w);
  }
  w->name = name;
  w->lat = la;
  w->lon = lo;
}

static void setup_user_waypoints(void) {
  static const struct { const char *name, *lat, *lon; } W[] = {
    {"Oilcamp", "36.68471", "-120.50277"},
    {"I5.Westshields", "36.77774", "-120.72426"},
    {"I5.165", "36.93022", "-120.84068"},
    {"I5.VOLTA", "37.01419", "-120.92878"},
    {"PT.ALPHA", "37.05665", "-120.96990"},
    {"Jellysferry", "N40 19.037", "W122 11.359"},
    {"Howie", "N40 21.893", "W122 13.042"},
    {"Hale", "N39 33.621", "W119 14.438"},
    {"Winnie", "N40 50.499", "W114 12.595"},
    {"WindRiver", "N42 43.733", "W108 38.800"},
    {"Buff", "N43 59.455", "W103 16.171"},
    {"Omega", "N44 53.388", "W95 38.935"},
    {"Paul", "N43 22.027", "W89 37.111"},
    {"MicrowaveSt", "N40 24.89", "W117 12.37"},
    {"RanchTower", "N41 6.16", "W115 5.43"},
    {"FremontIsland", "N41 10.49", "W112 20.64"},
    {"Tremonton", "N41 42.86", "W112 11.05"},
    {"RandomPoint", "N42", "W111 03"},
    {"Farson", "N42 6.40", "W109 26.95"},
    {"Midwest", "N43 24.49", "W109 16.68"},
    {"Bill", "N43 13.96", "W105 15.60"},
    {"MMNHS", "N43 52.67", "W101 57.65"},
    {"Tracks", "N44 21", "W100 22"},
    {"Towers", "N43 34.25", "W92 25.64"},
    {"IsletonBridge", "N38 10.32", "W121 35.62"},
    {"Mystery15", "N38 46.22", "W122 34.25"},
    {"Paskenta", "N39 53.13", "W122 32.36"},
    {"Bonanza", "N42 12.15", "W121 24.53"},
    {"Silverlake", "N43 07.41", "W121 03.74"},
    {"Millican", "N43 52.75", "W120 55.13"},
    {"Goering", "N44 05.751", "W120 56.834"},
    {"Constantia2", "N39 56.068", "W120 0.831"},
    {"Hallelujah2", "N39 46.509", "W120 2.336"},
    {"Redding.Pond", "N40 36", "W122 17"},
    {"Thunderhill", "N39 32.36", "W122 19.83"},
    {"CascadeHighway", "N40 46.63", "W122 19.12"},
    {"Eagleville", "N41 18.73", "W120 3.00"},
    {"DuckLakePass", "N41 3.00", "W120 3.00"},
  };
  table_clear(&user_waypoints);
  user_count = 0;
  for (size_t i = 0; i < sizeof W / sizeof W[0]; i++) user_update(W[i].name, W[i].lat, W[i].lon);
}

/* ---------- time (Time class) ---------- */
typedef struct { bool has; long s; } TimeV; /* has == false is JS undefined */
static TimeV T_of(long s) { TimeV t = {true, s}; return t; }
static TimeV T_undef(void) { TimeV t = {false, 0}; return t; }
static long T_need(TimeV t) {
  if (!t.has) die("undefined Time used");
  return t.s;
}
static double T_hours(TimeV t) { return (double)T_need(t) / 3600.0; }

static bool ci_eq(const char *a, const char *b, size_t n) {
  for (size_t i = 0; i < n; i++)
    if (toupper((unsigned char)a[i]) != toupper((unsigned char)b[i])) return false;
  return true;
}

/* TimeRE over p[0..n): ^(\d\d?)(?::([0-5]\d))?(?::([0-5]\d))?$ ; no match gives 0 */
static long time_from_text(const char *p, size_t n) {
  size_t i = 0;
  long g[3] = {0, 0, 0};
  int groups = 1;
  if (i >= n || !isdigit((unsigned char)p[i])) return 0;
  size_t l1 = (i + 1 < n && isdigit((unsigned char)p[i + 1])) ? 2 : 1;
  char b0[4] = {0};
  memcpy(b0, p, l1);
  g[0] = atol(b0);
  i = l1;
  for (int k = 1; k <= 2; k++) {
    if (i < n && p[i] == ':') {
      if (!(i + 3 <= n && p[i + 1] >= '0' && p[i + 1] <= '5' && isdigit((unsigned char)p[i + 2]))) return 0;
      g[k] = (p[i + 1] - '0') * 10 + (p[i + 2] - '0');
      i += 3;
      groups = k + 1;
    }
  }
  if (i != n) return 0;
  if (groups == 3) return (g[0] * 60 + g[1]) * 60 + g[2];
  if (groups == 2) return g[0] * 60 + g[1];
  return g[0];
}
static long time_from_str(const char *s) { return time_from_text(s, strlen(s)); }

/* Time.toString */
static void time_to_text(long secs, char *out) {
  char *o = out;
  long seconds = secs % 60;
  if (seconds < 0) { o += sprintf(o, "-"); seconds = -seconds; }
  long minutes = secs / 60;
  long hours = minutes / 60;
  minutes = minutes % 60;
  if (hours) o += sprintf(o, "%ld:", hours);
  if (minutes < 10 && hours) o += sprintf(o, "0");
  o += sprintf(o, "%ld:", minutes);
  if (seconds < 10) o += sprintf(o, "0");
  sprintf(o, "%ld", seconds);
}

/* whole-argument time check used by the TAXI/RUNUP/PATTERN/CLIMB/TAKEOFF time groups:
 * (\d\d?)(:[0-5]\d)? */
static bool time_arg_ok(const char *p, size_t n) {
  size_t i = 0;
  if (i >= n || !isdigit((unsigned char)p[i])) return false;
  i++;
  if (i < n && isdigit((unsigned char)p[i])) i++;
  if (i == n) return true;
  if (p[i] != ':') return false;
  return i + 3 == n && p[i + 1] >= '0' && p[i + 1] <= '5' && isdigit((unsigned char)p[i + 2]);
}

/* ---------- geometry (GeoLocation) ---------- */
typedef struct GeoLoc { double lat, lon; } GeoLoc;

typedef enum { K_PLAIN, K_START, K_TIMING, K_TAXI, K_RUNUP, K_TAKEOFF, K_CLIMB, K_PATTERN, K_TURN } Kind;
enum { EC_TAXI, EC_RUNUP, EC_TAKEOFF, EC_CLIMB, EC_CRUISE, EC_PATTERN, EC_COUNT };
static const double EC_FUEL[EC_COUNT] = {2, 8, 27, 22, 15, 11};
static const double EC_TAS[EC_COUNT] = {0, 0, 105, 125, 142, 95};

typedef struct FlightPlan FlightPlan;
typedef struct Leg {
  FlightPlan *fp;
  Kind kind;
  const char *type;   /* Leg.type; NULL for plain legs */
  const char *fix;
  char fixbuf[40];
  GeoLoc *location, *originalLocation;
  struct Leg *previous, *next;
  double course, distance, trueAirspeed, windDirection, windSpeed, heading, estGS, fuelFlow;
  double distanceRemaining, estFuel;
  bool startFlightTiming, stopFlightTiming, setLocFromPrev;
  int engineConfig, extraTurns;
  TimeV etE, estTimeRemaining, climbTime;
  char altitude[8];
} Leg;

struct FlightPlan {
  const char *name, *route;
  Leg *first, *last;
  double defWindDir, defWindSpeed, tasOverride;
  TimeV total, gate;
  Leg **legs; int nlegs, caplegs;
  GeoLoc **locs; int nlocs, caplocs;
};

/* RallyLeg.startLocation is a static on the class */
static GeoLoc *rally_start_location;

static GeoLoc *new_loc(FlightPlan *fp, double lat, double lon) {
  GeoLoc *g = malloc(sizeof *g);
  g->lat = lat; g->lon = lon;
  if (fp->nlocs == fp->caplocs) {
    fp->caplocs = fp->caplocs ? fp->caplocs * 2 : 16;
    fp->locs = realloc(fp->locs, fp->caplocs * sizeof *fp->locs);
  }
  fp->locs[fp->nlocs++] = g;
  return g;
}
static double geo_distance_to(const GeoLoc *a, const GeoLoc *b) {
  if (!a || !b) die("distance to undefined location");
  double dLat = to_rad(b->lat - a->lat);
  double dLon = to_rad(b->lon - a->lon);
  double h = sin(dLat / 2) * sin(dLat / 2) +
             cos(to_rad(a->lat)) * cos(to_rad(b->lat)) * sin(dLon / 2) * sin(dLon / 2);
  double c = 2 * atan2(sqrt(h), sqrt(1 - h));
  return EARTH_RADIUS * c;
}
static double geo_bearing_from(const GeoLoc *a, const GeoLoc *b) {
  if (!a || !b) die("bearing from undefined location");
  double dLon = to_rad(a->lon - b->lon);
  double thisLat = to_rad(a->lat), otherLat = to_rad(b->lat);
  double y = sin(dLon) * cos(to_rad(a->lat));
  double x = cos(otherLat) * sin(thisLat) - sin(otherLat) * cos(thisLat) * cos(dLon);
  return fmod(to_deg(atan2(y, x)) + 720 + 0, 360);
}
static GeoLoc *geo_location_from(FlightPlan *fp, const GeoLoc *a, double bearing, double distance) {
  if (!a) die("locationFrom undefined location");
  double bRad = to_rad(bearing - 0);
  double thisLat = to_rad(a->lat);
  double ang = distance / EARTH_RADIUS;
  double latRad = asin(sin(thisLat) * cos(ang) + cos(thisLat) * sin(ang) * cos(bRad));
  double lonRad = to_rad(a->lon) +
                  atan2(sin(bRad) * sin(ang) * cos(thisLat), cos(ang) - sin(thisLat) * sin(latRad));
  return new_loc(fp, to_deg(latRad), to_deg(lonRad));
}

/* ---------- legs (Leg, RallyLeg and subclasses) ---------- */
static Leg *new_leg(FlightPlan *fp, Kind kind, const char *type, const char *fix, GeoLoc *loc, int eng) {
  Leg *l = calloc(1, sizeof *l);
  l->fp = fp;
  l->kind = kind;
  l->type = type;
  l->fix = fix;
  l->location = loc;
  l->engineConfig = eng;
  l->etE = T_undef();
  l->estTimeRemaining = T_of(0);
  l->climbTime = T_undef();
  if (fp->nlegs == fp->caplegs) {
    fp->caplegs = fp->caplegs ? fp->caplegs * 2 : 32;
    fp->legs = realloc(fp->legs, fp->caplegs * sizeof *fp->legs);
  }
  fp->legs[fp->nlegs++] = l;
  return l;
}
static void set_fix_copy(Leg *l, const char *fix) {
  snprintf(l->fixbuf, sizeof l->fixbuf, "%s", fix);
  l->fix = l->fixbuf;
}

static void leg_update_distance_and_bearing(Leg *l, GeoLoc *other) {
  l->distance = geo_distance_to(l->location, other);
  l->course = js_round(geo_bearing_from(l->location, other));
  if (!l->etE.has && l->estGS != 0) l->etE = T_of((long)js_round(l->distance * 3600 / l->estGS));
  if (T_need(l->etE)) l->estFuel = l->fuelFlow * T_hours(l->etE);
}

static void leg_update_for_wind(Leg *l) {
  if (!l->windSpeed || !l->trueAirspeed) {
    l->heading = l->course;
    l->estGS = l->trueAirspeed;
    return;
  }
  double wdr = to_rad(l->windDirection);
  double cr = to_rad(l->course);
  double swc = (l->windSpeed / l->trueAirspeed) * sin(wdr - cr);
  if (fabs(swc) > 1) { status("Wind to strong to fly!"); return; }
  double hr = cr + asin(swc);
  if (hr < 0) hr += TWO_PI_JS;
  if (hr > TWO_PI_JS) hr -= TWO_PI_JS;
  double gs = l->trueAirspeed * sqrt(1 - swc * swc) - l->windSpeed * cos(wdr - cr);
  if (gs < 0) { status("Wind to strong to fly!"); return; }
  l->estGS = gs;
  l->heading = js_round(to_deg(hr));
}

static void leg_calculate(Leg *l) {
  if (!l->trueAirspeed) l->trueAirspeed = EC_TAS[l->engineConfig];
  l->fuelFlow = EC_FUEL[l->engineConfig];
  leg_update_for_wind(l);
}

static bool type_is(const char *t, const char *want) { return t && !strcmp(t, want); }

static void leg_update_forward(Leg *l) {
  FlightPlan *fp = l->fp;
  Leg *prev = l->previous;
  bool havePrevious = true;
  if (!prev) {
    havePrevious = false;
    prev = l;
    if (!l->etE.has) l->etE = T_of(0);
  }
  const char *thisType = l->type;
  if (type_is(thisType, "Climb") && havePrevious) {
    l->location = prev->location;
    return;
  }
  leg_update_distance_and_bearing(l, prev->location);
  leg_update_for_wind(l);
  Leg *next = l->next;
  if (!havePrevious) return;
  if (type_is(prev->type, "Climb")) {
    double climbDistance = prev->estGS * T_hours(prev->climbTime);
    if (climbDistance < l->distance) {
      GeoLoc *climbStart = prev->location;
      GeoLoc *climbEnd = geo_location_from(fp, climbStart, l->course, climbDistance);
      prev->location = climbEnd;
      leg_update_distance_and_bearing(prev, climbStart);
      l->etE = T_undef();
      leg_update_distance_and_bearing(l, climbEnd);
    } else {
      status("Not enough distance to climb");
    }
  } else if ((type_is(thisType, "Left") || type_is(thisType, "Right")) && next && next->location) {
    bool left = type_is(thisType, "Left");
    double standardRateCircumference = l->trueAirspeed / 30;
    double standardRateRadius = standardRateCircumference / TWO_PI_JS;
    double offsetInbound = 360 + prev->course + (left ? -90 : 90);
    offsetInbound = js_round(fmod(offsetInbound + 360, 360));
    if (!prev->originalLocation) prev->originalLocation = prev->location;
    GeoLoc *previousLocation = prev->originalLocation;
    GeoLoc *inbound = geo_location_from(fp, previousLocation, offsetInbound, standardRateRadius);
    double bearingToNext = js_round(geo_bearing_from(next->location, previousLocation));
    double offsetOutbound = bearingToNext + (left ? 90 : -90);
    offsetOutbound = fmod(offsetOutbound + 360, 360);
    GeoLoc *outbound = geo_location_from(fp, previousLocation, offsetOutbound, standardRateRadius);
    double turnAngle = left ? (360 + bearingToNext - prev->course) : (360 + prev->course - bearingToNext);
    turnAngle = fmod(turnAngle + 360, 360);
    l->etE = T_of((long)js_round((turnAngle + 360 * l->extraTurns) / 3));
    l->estFuel = l->fuelFlow * T_hours(l->etE);
    l->location = outbound;
    l->distance = l->trueAirspeed * T_hours(l->etE);
    prev->location = inbound;
    Leg *prevPrev = prev->previous;
    if (prevPrev && prevPrev->location) {
      prev->etE = T_undef();
      leg_update_distance_and_bearing(prev, prevPrev->location);
    }
  }
}

static void leg_update_backward(Leg *l) {
  Leg *next = l->next;
  double distanceRemaining = 0;
  TimeV timeRemaining = T_of(0);
  if (next) {
    distanceRemaining = next->distanceRemaining;
    timeRemaining = next->estTimeRemaining;
  }
  if (l->stopFlightTiming || T_need(timeRemaining)) {
    l->distanceRemaining = distanceRemaining + l->distance;
    l->estTimeRemaining = T_of(timeRemaining.s + T_need(l->etE));
  } else {
    l->estTimeRemaining = T_of(0);
  }
}

/* Leg.fixName / Leg.toString, appended to out */
static void leg_name_into(const Leg *l, char *o) {
  char t[32];
  switch (l->kind) {
  case K_PLAIN:
    strcat(o, l->fix);
    break;
  case K_START:
    strcat(o, l->fix); strcat(o, "|Start");
    break;
  case K_TIMING:
    strcat(o, l->fix); strcat(o, "|Timing");
    break;
  case K_TAXI:
    time_to_text(T_need(l->etE), t);
    strcat(o, "Taxi|"); strcat(o, t);
    break;
  case K_RUNUP:
    time_to_text(T_need(l->etE), t);
    strcat(o, "Runup|"); strcat(o, t);
    break;
  case K_TAKEOFF:
    strcat(o, "Takeoff");
    if (T_need(l->etE) != 120) {
      time_to_text(T_need(l->etE), t);
      strcat(o, "|"); strcat(o, t);
    }
    break;
  case K_CLIMB:
    time_to_text(T_need(l->etE), t);
    strcat(o, "Climb|"); strcat(o, l->altitude); strcat(o, "|"); strcat(o, t);
    break;
  case K_PATTERN:
    time_to_text(T_need(l->etE), t);
    strcat(o, "Pattern|"); strcat(o, t);
    break;
  case K_TURN:
    strcat(o, l->type);
    if (l->extraTurns) { sprintf(t, "|+%d", l->extraTurns); strcat(o, t); }
    break;
  }
}

/* ---------- route tokens (regex equivalents) ---------- */
static const char *KW[7] = {"TAXI", "RUNUP", "TAKEOFF", "CLIMB", "PATTERN", "LEFT", "RIGHT"};
enum { KW_TAXI, KW_RUNUP, KW_TAKEOFF, KW_CLIMB, KW_PATTERN, KW_LEFT, KW_RIGHT };

/* RallyLegNoFixRE searched (not anchored) in s[0..n) */
static bool find_kw(const char *s, size_t n, int *which) {
  for (size_t i = 0; i < n; i++)
    for (int k = 0; k < 7; k++) {
      size_t kl = strlen(KW[k]);
      if (i + kl <= n && ci_eq(s + i, KW[k], kl)) { *which = k; return true; }
    }
  return false;
}

/* RallyLegWithFixRE = /^([0-9a-z.]{3,16})\|(START|TIMING)/i ; returns the fix length or 0 */
static size_t rally_fix_len(const char *t) {
  size_t L = 0;
  while (isalnum((unsigned char)t[L]) || t[L] == '.') L++;
  if (L < 3 || L > 16 || t[L] != '|') return 0;
  if (!ci_eq(t + L + 1, "START", 5) && !ci_eq(t + L + 1, "TIMING", 6)) return 0;
  return L;
}

/* LegModifier = /(360|3[0-5][0-9]|[0-2][0-9]{2}|[0-9]{1,2})@([0-9]{1,3})|([1-9][0-9]{1,2}|0)kts/i
 * Returns 0 (no match), 1 (wind: a = direction, b = speed) or 2 (true airspeed: a). */
static int leg_modifier(const char *t, long *a, long *b) {
  size_t n = strlen(t);
  for (size_t i = 0; i < n; i++) {
    if (isdigit((unsigned char)t[i])) {
      size_t cand[4];
      int nc = 0;
      if (i + 3 <= n && !strncmp(t + i, "360", 3)) cand[nc++] = 3;
      if (i + 3 <= n && t[i] == '3' && t[i + 1] >= '0' && t[i + 1] <= '5' && isdigit((unsigned char)t[i + 2])) cand[nc++] = 3;
      if (i + 3 <= n && t[i] >= '0' && t[i] <= '2' && isdigit((unsigned char)t[i + 1]) && isdigit((unsigned char)t[i + 2])) cand[nc++] = 3;
      if (i + 2 <= n && isdigit((unsigned char)t[i + 1])) cand[nc++] = 2;
      cand[nc++] = 1;
      for (int c = 0; c < nc; c++) {
        size_t L = cand[c];
        if (i + L >= n || t[i + L] != '@') continue;
        size_t j = i + L + 1;
        if (j >= n || !isdigit((unsigned char)t[j])) continue;
        size_t k = j;
        while (k < n && k < j + 3 && isdigit((unsigned char)t[k])) k++;
        char b0[4] = {0}, b1[4] = {0};
        memcpy(b0, t + i, L);
        memcpy(b1, t + j, k - j);
        *a = atol(b0);
        *b = atol(b1);
        return 1;
      }
    }
    /* ([1-9][0-9]{1,2}|0)kts */
    size_t cand[3];
    int nc = 0;
    if (t[i] >= '1' && t[i] <= '9') {
      if (i + 3 <= n && isdigit((unsigned char)t[i + 1]) && isdigit((unsigned char)t[i + 2])) cand[nc++] = 3;
      if (i + 2 <= n && isdigit((unsigned char)t[i + 1])) cand[nc++] = 2;
    }
    if (t[i] == '0') cand[nc++] = 1;
    for (int c = 0; c < nc; c++) {
      size_t L = cand[c];
      if (i + L + 3 <= n && ci_eq(t + i + L, "KTS", 3)) {
        char b0[4] = {0};
        memcpy(b0, t + i, L);
        *a = atol(b0);
        return 2;
      }
    }
  }
  return 0;
}

/* TAXI|RUNUP style: ^KW(?:\|(time))?$ ; returns 0 no match, 1 no time, 2 time in (tp, tn) */
static int match_kw_opt_time(const char *t, const char *kw, const char **tp, size_t *tn) {
  size_t kl = strlen(kw);
  if (strncmp(t, kw, kl)) return 0;
  const char *r = t + kl;
  if (!*r) return 1;
  if (*r != '|') return 0;
  size_t n = strlen(r + 1);
  if (!time_arg_ok(r + 1, n)) return 0;
  *tp = r + 1;
  *tn = n;
  return 2;
}

/* CLIMB|(\d{3,5})|(time) ; fills alt and the time */
static bool match_climb(const char *t, char *alt, const char **tp, size_t *tn) {
  if (strncmp(t, "CLIMB|", 6)) return false;
  const char *p = t + 6;
  size_t L = 0;
  while (isdigit((unsigned char)p[L])) L++;
  if (L < 3 || L > 5 || p[L] != '|') return false;
  const char *tt = p + L + 1;
  size_t n = strlen(tt);
  if (!time_arg_ok(tt, n)) return false;
  memcpy(alt, p, L);
  alt[L] = 0;
  *tp = tt;
  *tn = n;
  return true;
}

/* ^TURN-word(?:\|\+(\d))?$ with the keyword LEFT or RIGHT */
static bool match_turn(const char *t, bool *left, int *extra) {
  size_t kl;
  if (!strncmp(t, "LEFT", 4)) { kl = 4; *left = true; }
  else if (!strncmp(t, "RIGHT", 5)) { kl = 5; *left = false; }
  else return false;
  const char *r = t + kl;
  if (!*r) { *extra = 0; return true; }
  if (r[0] == '|' && r[1] == '+' && isdigit((unsigned char)r[2]) && !r[3]) { *extra = r[2] - '0'; return true; }
  return false;
}

/* ---------- leg constructors ---------- */
static Leg *taxi_leg(FlightPlan *fp, const char *text) {
  const char *tp = NULL;
  size_t tn = 0;
  int m = match_kw_opt_time(text, "TAXI", &tp, &tn);
  if (!m) die("TaxiLeg: text does not match TAXI[|time]");
  Leg *l = new_leg(fp, K_TAXI, "Taxi", "", new_loc(fp, -1, -1), EC_TAXI);
  l->etE = (m == 2) ? T_of(time_from_text(tp, tn)) : T_of(time_from_str("5:00"));
  l->setLocFromPrev = true;
  return l;
}
static Leg *runup_leg(FlightPlan *fp, const char *text) {
  const char *tp = NULL;
  size_t tn = 0;
  int m = match_kw_opt_time(text, "RUNUP", &tp, &tn);
  if (!m) die("RunupLeg: text does not match RUNUP[|time]");
  Leg *l = new_leg(fp, K_RUNUP, "Runup", "", new_loc(fp, -1, -1), EC_RUNUP);
  l->etE = (m == 2) ? T_of(time_from_text(tp, tn)) : T_of(time_from_str("30"));
  l->setLocFromPrev = true;
  return l;
}
static Leg *takeoff_leg(FlightPlan *fp, const char *text) {
  /* TAKEOFF[|time] ; the bearing@distance form is caught by LegModifier before it can reach here */
  const char *r = text + 7;
  TimeV t = T_of(120);
  if (*r == '|') {
    size_t n = strlen(r + 1);
    if (!time_arg_ok(r + 1, n)) die("TakeoffLeg: unsupported TAKEOFF form");
    t = T_of(time_from_text(r + 1, n));
  } else if (*r) {
    die("TakeoffLeg: unsupported TAKEOFF form");
  }
  Leg *l = new_leg(fp, K_TAKEOFF, "Takeoff", "", rally_start_location, EC_TAKEOFF);
  l->etE = t;
  l->startFlightTiming = true;
  return l;
}
static Leg *climb_leg(FlightPlan *fp, const char *text) {
  char alt[8] = "5500";
  const char *tp = "8:00";
  size_t tn = 4;
  match_climb(text, alt, &tp, &tn);
  Leg *l = new_leg(fp, K_CLIMB, "Climb", "", NULL, EC_CLIMB);
  memcpy(l->altitude, alt, sizeof alt);
  l->etE = T_of(time_from_text(tp, tn));
  l->climbTime = l->etE;
  l->setLocFromPrev = true;
  return l;
}
static Leg *pattern_leg(FlightPlan *fp, const char *text) {
  const char *tp = NULL;
  size_t tn = 0;
  if (strncmp(text, "PATTERN|", 8)) die("PatternLeg: text does not match PATTERN|time");
  if (!time_arg_ok(text + 8, strlen(text + 8))) die("PatternLeg: text does not match PATTERN|time");
  tp = text + 8;
  tn = strlen(tp);
  Leg *l = new_leg(fp, K_PATTERN, "Pattern", "", NULL, EC_PATTERN);
  l->etE = T_of(time_from_text(tp, tn));
  l->setLocFromPrev = true;
  return l;
}
static Leg *turn_leg(FlightPlan *fp, const char *text) {
  bool left = true;
  int extra = 0;
  bool ok = match_turn(text, &left, &extra);
  const char *dir = (ok && !left) ? "Right" : "Left";
  Leg *l = new_leg(fp, K_TURN, dir, "", new_loc(fp, -1, -1), EC_CRUISE);
  l->extraTurns = ok ? extra : 0;
  return l;
}

/* RallyLegWithoutFix.getLegNoFix */
static Leg *get_leg_no_fix(FlightPlan *fp, const char *text) {
  size_t n = strcspn(text, "|");
  char first[160];
  if (n >= sizeof first) die("route token too long");
  for (size_t i = 0; i < n; i++) first[i] = (char)toupper((unsigned char)text[i]);
  first[n] = 0;
  int which;
  if (!find_kw(first, n, &which)) return NULL;
  switch (which) {
  case KW_TAXI: return taxi_leg(fp, text);
  case KW_RUNUP: return runup_leg(fp, text);
  case KW_TAKEOFF:
    if (!rally_start_location) { status("Trying to create a Takeoff leg without start leg"); return NULL; }
    return takeoff_leg(fp, text);
  case KW_CLIMB: return climb_leg(fp, text);
  case KW_PATTERN: return pattern_leg(fp, text);
  default: return turn_leg(fp, text);
  }
}

/* RallyLeg.getLegWithFix */
static Leg *get_leg_with_fix(FlightPlan *fp, const char *text, const char *fix, GeoLoc *loc) {
  size_t L = rally_fix_len(text);
  const char *legType = text + L + 1;
  if (!strncmp(legType, "START", 5)) {
    if (rally_start_location) { status("Trying to create second start leg"); return NULL; }
    rally_start_location = loc;
    Leg *l = new_leg(fp, K_START, "Start", "", loc, EC_TAXI);
    set_fix_copy(l, fix);
    return l;
  }
  if (!strncmp(legType, "TIMING", 6)) {
    Leg *l = new_leg(fp, K_TIMING, "Timing", "", loc, EC_CRUISE);
    set_fix_copy(l, fix);
    l->stopFlightTiming = true;
    return l;
  }
  js_error("Unhandled Rally Leg type");
  return NULL;
}

/* ---------- flight plan ---------- */
static void fp_append(FlightPlan *fp, Leg *leg) {
  if (!fp->first) fp->first = leg;
  if (fp->last) fp->last->next = leg;
  /* RallyLegWithoutFix.setPrevious copies the previous location for some kinds */
  if (leg->setLocFromPrev && fp->last) leg->location = fp->last->location;
  leg->previous = fp->last;
  leg->next = NULL;
  if (fp->tasOverride) {
    leg->trueAirspeed = fp->tasOverride;
    fp->tasOverride = 0;
  }
  if (fp->defWindSpeed) {
    leg->windDirection = fp->defWindDir;
    leg->windSpeed = fp->defWindSpeed;
  }
  fp->last = leg;
}

static FlightPlan *fp_new(const char *name, const char *route) {
  FlightPlan *fp = calloc(1, sizeof *fp);
  fp->name = name;
  fp->route = route;
  fp->total = T_of(0);
  fp->gate = T_of(0);
  rally_start_location = NULL; /* RallyLeg.reset() */
  return fp;
}
static void fp_destroy(FlightPlan *fp) {
  for (int i = 0; i < fp->nlegs; i++) free(fp->legs[i]);
  for (int i = 0; i < fp->nlocs; i++) free(fp->locs[i]);
  free(fp->legs);
  free(fp->locs);
  free(fp);
}

static void resolve_waypoint(FlightPlan *fp, const char *text) {
  long a = 0, b = 0;
  int mod = leg_modifier(text, &a, &b);
  if (mod == 1) {
    /* setDefaultWind(parseInt(match[1]) % 360, parseInt(match[2])) */
    fp->defWindDir = (double)(a % 360);
    fp->defWindSpeed = (double)b;
    return;
  }
  if (mod == 2) {
    fp->tasOverride = (double)a;
    return;
  }
  /* RallyLegWithoutFix.isRallyLegWithoutFix: unanchored keyword search in the part before '|' */
  size_t recLen = strcspn(text, "|");
  int which;
  if (find_kw(text, recLen, &which)) {
    Leg *l = get_leg_no_fix(fp, text);
    if (l) fp_append(fp, l);
    return;
  }
  size_t L = rally_fix_len(text);
  bool isRally = L > 0;
  char fixname[32];
  const char *fixName;
  if (isRally) {
    memcpy(fixname, text, L);
    fixname[L] = 0;
    fixName = fixname;
  } else {
    fixName = text;
  }
  Waypoint *wp = table_get(&user_waypoints, fixName);
  if (!wp) wp = table_get(&faa_waypoints, fixName);
  if (!wp) {
    char msg[256];
    snprintf(msg, sizeof msg, "Couldn't find waypoint \"%s\"", text);
    js_error(msg);
    return;
  }
  GeoLoc *loc = new_loc(fp, wp->lat, wp->lon);
  if (isRally) {
    Leg *l = get_leg_with_fix(fp, text, fixName, loc);
    if (!l) die("rally leg with fix could not be created");
    fp_append(fp, l);
  } else {
    fp_append(fp, new_leg(fp, K_PLAIN, NULL, wp->name, loc, EC_CRUISE));
  }
}

/* FaaAirways.resolveAirway: returns the fixes from entry to exit (inclusive), 0 when none */
static int resolve_airway(const char *id, const char *entry, const char *exit, const char **out) {
  Airway *a = table_get(&faa_airways, id);
  if (!a) return 0;
  int ei = -1, xi = -1;
  for (int i = 0; i < a->nfix; i++) {
    if (ei < 0 && !strcmp(a->fix[i], entry)) ei = i;
    if (xi < 0 && !strcmp(a->fix[i], exit)) xi = i;
  }
  if (ei == -1 || xi == -1) return 0;
  int stride = (ei <= xi) ? 1 : -1;
  int n = 0;
  for (int idx = ei; idx != xi; idx += stride) out[n++] = a->fix[idx];
  out[n++] = a->fix[xi];
  return n;
}

static void fp_parse_route(FlightPlan *fp) {
  char buf[1024];
  if (strlen(fp->route) >= sizeof buf) die("route too long");
  strcpy(buf, fp->route);
  /* route.split(/ +/) */
  const char *tok[512];
  int ntok = 0;
  char *p = buf;
  for (;;) {
    char *start = p;
    while (*p && *p != ' ') p++;
    tok[ntok++] = start;
    if (!*p) break;
    *p++ = 0;
    while (*p == ' ') p++;
  }
  char prior[160] = "";
  for (int i = 0; i < ntok; i++) {
    char up[160];
    size_t n = strlen(tok[i]);
    if (n >= sizeof up) die("route token too long");
    for (size_t k = 0; k <= n; k++) up[k] = (char)toupper((unsigned char)tok[i][k]);
    const char *airwayFixes[512];
    int nf = 0;
    if (table_get(&faa_airways, up) && i + 1 < ntok) {
      /* the exit fix is used exactly as written (not uppercased), as in parseRoute */
      nf = resolve_airway(up, prior, tok[i + 1], airwayFixes);
      for (int k = 1; k < nf - 1; k++) resolve_waypoint(fp, airwayFixes[k]);
    } else {
      resolve_waypoint(fp, up);
    }
    strcpy(prior, up);
  }
}

static void fp_calculate(FlightPlan *fp) {
  if (!fp->first) return;
  bool haveStart = false, haveStop = false;
  for (Leg *l = fp->first; l; l = l->next) {
    leg_calculate(l);
    if (l->startFlightTiming) { if (haveStart) status("duplicate Start timing leg"); haveStart = true; }
    if (l->stopFlightTiming) { if (haveStop) status("duplicate Timing leg"); haveStop = true; }
  }
  if (!haveStart) fp->first->startFlightTiming = true;
  if (!haveStop) fp->last->stopFlightTiming = true;
  for (Leg *l = fp->first; l; l = l->next) leg_update_forward(l);
  for (Leg *l = fp->last; l; l = l->previous) leg_update_backward(l);
  for (Leg *l = fp->first; l; l = l->next)
    if (l->startFlightTiming) fp->gate = l->estTimeRemaining;
  fp->total = fp->first->estTimeRemaining;
}

static void fp_resolved_route(const FlightPlan *fp, char *out) {
  out[0] = 0;
  double lastDir = 0, lastSpeed = 0;
  int idx = 0;
  char t[32];
  for (const Leg *l = fp->first; l; l = l->next, idx++) {
    if (idx) strcat(out, " ");
    if (!(l->windDirection == lastDir && l->windSpeed == lastSpeed)) {
      if (l->windSpeed) {
        sprintf(t, "%lld@%lld", l->windDirection ? (long long)l->windDirection : 360LL, (long long)l->windSpeed);
        strcat(out, t);
      }
      strcat(out, " ");
      lastDir = l->windDirection;
      lastSpeed = l->windSpeed;
    }
    if (!(l->trueAirspeed == EC_TAS[l->engineConfig])) {
      sprintf(t, "%lldkts ", (long long)l->trueAirspeed);
      strcat(out, t);
    }
    leg_name_into(l, out);
  }
}

/* ---------- expected flight plans (expectations.js) ---------- */
typedef struct { char *name, *route, *expRoute; TimeV total, gate; FlightPlan *fp; } Expected;
static Expected *expected;
static int n_expected;

static void skip_sp(Scan *s) { ws(s); }
static char *expr_string(Scan *s) {
  char *acc = calloc(1, 1);
  size_t n = 0;
  for (;;) {
    skip_sp(s);
    char *term;
    if (peek_is(s, '"')) {
      term = scan_string(s);
    } else if (s->end - s->p > 13 && !strncmp(s->p, "keywords.get(", 13)) {
      s->p += 13;
      term = scan_string(s);
      expect(s, ')');
    } else {
      die("unsupported expression in expectations.js");
    }
    size_t tl = strlen(term);
    acc = realloc(acc, n + tl + 1);
    memcpy(acc + n, term, tl + 1);
    n += tl;
    free(term);
    skip_sp(s);
    if (peek_is(s, '+')) { s->p++; continue; }
    break;
  }
  return acc;
}
static TimeV expr_time(Scan *s) {
  skip_sp(s);
  if (s->end - s->p >= 9 && !strncmp(s->p, "undefined", 9)) { s->p += 9; return T_undef(); }
  if (s->end - s->p >= 9 && !strncmp(s->p, "new Time(", 9)) {
    s->p += 9;
    char *t = scan_string(s);
    expect(s, ')');
    TimeV v = T_of(time_from_str(t));
    free(t);
    return v;
  }
  die("unsupported time expression in expectations.js");
  return T_undef();
}

static void load_expectations(const char *text, size_t len) {
  Scan s = {text, text + len};
  const char *tag = "new ExpectedFlightPlan(";
  size_t tl = strlen(tag);
  int cap = 0;
  for (;;) {
    const char *hit = NULL;
    for (const char *q = s.p; q + tl <= s.end; q++)
      if (!strncmp(q, tag, tl)) { hit = q; break; }
    if (!hit) break;
    s.p = hit + tl;
    if (n_expected == cap) {
      cap = cap ? cap * 2 : 128;
      expected = realloc(expected, cap * sizeof *expected);
    }
    Expected *e = &expected[n_expected++];
    e->name = expr_string(&s);
    expect(&s, ',');
    e->route = expr_string(&s);
    expect(&s, ',');
    e->expRoute = expr_string(&s);
    expect(&s, ',');
    e->total = expr_time(&s);
    expect(&s, ',');
    e->gate = expr_time(&s);
    expect(&s, ')');
    e->fp = NULL;
  }
}

/* ---------- benchmark ---------- */
static void run_iteration(void) {
  for (int i = 0; i < 5; i++) {
    setup_user_waypoints();
    for (int j = 0; j < n_expected; j++) {
      Expected *e = &expected[j];
      if (e->fp) fp_destroy(e->fp);           /* reset() */
      e->fp = fp_new(e->name, e->route);
      fp_parse_route(e->fp);                  /* resolveRoute() */
    }
  }
}

static int check_expectations(const Expected *e) {
  int bad = 0;
  char buf[4096], t1[32], t2[32];
  if (e->expRoute[0]) {
    buf[0] = 0;
    fp_resolved_route(e->fp, buf);
    if (strcmp(e->expRoute, buf)) {
      fprintf(stderr, "Error: Flight plan %s route different than expected (\"%s\"), got (\"%s\")\n",
              e->name, e->expRoute, buf);
      bad++;
    }
  }
  if (e->total.has) {
    long delta = labs(e->total.s - T_need(e->fp->total));
    if (delta > 5) {
      time_to_text(e->total.s, t1);
      time_to_text(T_need(e->fp->total), t2);
      fprintf(stderr, "Error: Flight plan %s total time different than expected (%s), got (%s)\n", e->name, t1, t2);
      bad++;
    }
  }
  if (e->gate.has) {
    long delta = labs(e->gate.s - T_need(e->fp->gate));
    if (delta > 5) {
      time_to_text(e->gate.s, t1);
      time_to_text(T_need(e->fp->gate), t2);
      fprintf(stderr, "Error: Flight plan %s gate time different than expected (%s), got (%s)\n", e->name, t1, t2);
      bad++;
    }
  }
  return bad;
}

static int validate(void) {
  int bad = 0;
  for (int i = 0; i < n_expected; i++) {
    fp_calculate(expected[i].fp);
    bad += check_expectations(&expected[i]);
  }
  return bad;
}

int main(int argc, char **argv) {
  const char *root = argc > 2 ? argv[2] : "quench-bench/jetstream";
  char path[1024];

  table_init(&faa_waypoints, 1 << 18);
  table_init(&faa_airways, 1 << 11);
  table_init(&user_waypoints, 1 << 7);

  snprintf(path, sizeof path, "%s/RexBench/FlightPlanner/airways.js", root);
  size_t n;
  unsigned char *airways = read_file(path, &n);
  load_airways((const char *)airways, n);
  free(airways);

  snprintf(path, sizeof path, "%s/RexBench/FlightPlanner/waypoints.js.z", root);
  load_waypoints_z(path);

  snprintf(path, sizeof path, "%s/RexBench/FlightPlanner/expectations.js", root);
  unsigned char *exp = read_file(path, &n);
  load_expectations((const char *)exp, n);
  free(exp);

  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) run_iteration();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("FlightPlanner %d iterations %.3f ms\n", iterations,
         (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);

  int bad = validate();
  if (bad) {
    printf("validation: FAILED (%d mismatches)\n", bad);
    return 1;
  }
  printf("validation: OK (%d flight plans)\n", n_expected);
  return 0;
}
