// jetstream-row: prismjs-startup-es6
// jetstream-files: utils/StartupBenchmark.js prismjs/benchmark.js prismjs/dist/bundle.es6.min.js prismjs/data/sample.cpp prismjs/data/sample.css prismjs/data/sample.html prismjs/data/sample.js prismjs/data/sample.json prismjs/data/sample.md prismjs/data/sample.py prismjs/data/sample.sql prismjs/data/sample.ts
// iterations: 120
/* C analogue of JetStream's prismjs-startup-es6 row.
 *
 * The JS row re-evaluates the Prism bundle each iteration (a fresh Prism.languages, so the
 * grammars are rebuilt, and rest-merges happen again) and then highlights eight samples.
 * This analogue does the same work inside the timed loop:
 *   1. build all grammar objects: compile every grammar regex (a small backtracking regex
 *      engine that understands the JS syntax the grammars use), build the entry lists and
 *      the inside/alias links. prismjs-grammars.h holds the grammar object graph generated
 *      from the bundle's own regex sources and key order, so no regex text is retyped;
 *   2. run Prism.highlight on the eight samples: a port of Prism's tokenize/matchGrammar
 *      (greedy matching, lookbehind group, nested inside grammars, rematch of greedy
 *      matches), the rest-merge done on first tokenize, and the stringify/encode step;
 *   3. the hooks the samples reach: markdown after-tokenize (code-block language alias),
 *      wrap code-block (re-highlights fenced code), wrap entity (title attribute);
 *   4. quickHash of each html, checked against the constants in prism benchmark.js.
 * Not included: the JS bundle's parse and eval (the C grammar data is compiled in).
 * Usage: prismjs-startup-es6 [jetstream-root] [iterations]
 *   jetstream-root defaults to ../jetstream; iterations defaults to 120. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "prismjs-grammars.h"

#define MAXCAP 32

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

/* ---- arena: everything built in one iteration lives until the next reset ---- */
typedef struct Chunk { struct Chunk *next; size_t cap, used; char data[]; } Chunk;
static Chunk *ar_head;
static Chunk *ar_cur;

static void *ar_alloc(size_t n) {
  n = (n + 15) & ~(size_t)15;
  if (!ar_cur || ar_cur->used + n > ar_cur->cap) {
    Chunk *c = ar_cur && ar_cur->next ? ar_cur->next : NULL;
    if (!c) {
      size_t cap = n > (1u << 20) ? n : (1u << 20);
      c = malloc(sizeof(Chunk) + cap);
      c->next = NULL; c->cap = cap; c->used = 0;
      if (ar_cur) ar_cur->next = c; else ar_head = c;
    }
    c->used = 0;
    ar_cur = c;
  }
  void *p = ar_cur->data + ar_cur->used;
  ar_cur->used += n;
  memset(p, 0, n);
  return p;
}
static void ar_reset(void) { ar_cur = ar_head; if (ar_cur) ar_cur->used = 0; }

/* ---- backtracking regex engine (JS syntax subset used by the grammars) ---- */
enum { N_SET, N_BOL, N_EOL, N_WB, N_DOT, N_BREF, N_GROUP, N_ALT, N_LOOK, N_QUANT };
typedef struct Node Node;
struct Node {
  int kind, neg, multi, dotall, icase;
  int min, max, greedy;        /* QUANT */
  int cap;                     /* GROUP capture index (0 = non-capturing), BREF index */
  int cap_lo, cap_hi;          /* QUANT: capture groups inside the atom */
  uint8_t set[32];             /* SET */
  Node *next, *child;          /* child: GROUP, LOOK, QUANT */
  Node **alts; int nalts;      /* ALT */
};
typedef struct Rx { Node *root; int known; uint8_t first[32]; } Rx;

typedef struct { const char *p, *e; int icase, multi, dotall, ncap; const char *src; } RP;

static void rx_fail(const char *msg, const char *src) {
  fprintf(stderr, "regex: %s in /%s/\n", msg, src);
  exit(4);
}
static Node *new_node(int kind) { Node *n = ar_alloc(sizeof(Node)); n->kind = kind; return n; }
static void set_add(uint8_t *s, int c) { if (c >= 0 && c < 256) s[c >> 3] |= (uint8_t)(1u << (c & 7)); }
static int set_has(const uint8_t *s, int c) { return c >= 0 && c < 256 && ((s[c >> 3] >> (c & 7)) & 1); }
static void set_negate(uint8_t *s) { for (int i = 0; i < 32; i++) s[i] = (uint8_t)~s[i]; }
static void set_or(uint8_t *d, const uint8_t *s) { for (int i = 0; i < 32; i++) d[i] |= s[i]; }
static int is_word(int c) { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_'; }
static int is_lt(int c) { return c == '\n' || c == '\r'; }
static int lower(int c) { return c >= 'A' && c <= 'Z' ? c + 32 : c; }

/* Adds the class \d \w \s (upper-case: complement) to a bitmap. */
static void class_escape(uint8_t *bits, int c) {
  uint8_t t[32] = {0};
  switch (lower(c)) {
    case 'd': for (int k = '0'; k <= '9'; k++) set_add(t, k); break;
    case 'w': for (int k = 0; k < 256; k++) if (is_word(k)) set_add(t, k); break;
    case 's': for (int k = 9; k <= 13; k++) set_add(t, k); set_add(t, 32); set_add(t, 0xA0); break;
  }
  if (c >= 'A' && c <= 'Z') set_negate(t);
  set_or(bits, t);
}
static int hexdig(int c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  return -1;
}
/* Reads the character named by an escape after the backslash (r->p points past '\'). */
static int char_escape(RP *r) {
  int c = *r->p++;
  switch (c) {
    case 'f': return 12;
    case 'n': return 10;
    case 'r': return 13;
    case 't': return 9;
    case 'v': return 11;
    case 'b': return 8;
    case '0': return 0;
    case 'x': {
      int v = hexdig(r->p[0]), w = hexdig(r->p[1]);
      if (v < 0 || w < 0) rx_fail("bad \\x", r->src);
      r->p += 2; return v * 16 + w;
    }
    case 'u': {
      int v = 0;
      for (int k = 0; k < 4; k++) { int d = hexdig(r->p[k]); if (d < 0) rx_fail("bad \\u", r->src); v = v * 16 + d; }
      r->p += 4; return v;
    }
    case 'c': { int l = *r->p++; return l % 32; }
    default: return c; /* identity escape */
  }
}
/* One class item: returns its code, or -1 when it was a class escape added to bits. */
static int class_atom(RP *r, uint8_t *bits) {
  if (*r->p == '\\') {
    r->p++;
    int c = *r->p;
    if (c == 'd' || c == 'D' || c == 's' || c == 'S' || c == 'w' || c == 'W') {
      r->p++; class_escape(bits, c); return -1;
    }
    return char_escape(r);
  }
  return *r->p++;
}
static void fold_case(uint8_t *bits) {
  for (int c = 'a'; c <= 'z'; c++) {
    if (set_has(bits, c) || set_has(bits, c - 32)) { set_add(bits, c); set_add(bits, c - 32); }
  }
}
static Node *parse_class(RP *r) {
  uint8_t bits[32] = {0};
  int neg = 0;
  if (*r->p == '^') { neg = 1; r->p++; }
  while (r->p < r->e && *r->p != ']') {
    int c1 = class_atom(r, bits);
    if (c1 >= 0 && r->p + 1 < r->e && *r->p == '-' && r->p[1] != ']') {
      r->p++;
      int c2 = class_atom(r, bits);
      if (c2 < 0) { set_add(bits, c1); set_add(bits, '-'); }
      else for (int c = c1; c <= c2 && c < 256; c++) set_add(bits, c);
    } else if (c1 >= 0) {
      set_add(bits, c1);
    }
  }
  if (r->p >= r->e) rx_fail("unterminated class", r->src);
  r->p++;
  if (r->icase) fold_case(bits);
  if (neg) set_negate(bits);
  Node *n = new_node(N_SET);
  memcpy(n->set, bits, 32);
  return n;
}
static Node *single_char(int c, int icase) {
  Node *n = new_node(N_SET);
  set_add(n->set, c);
  if (icase) fold_case(n->set);
  return n;
}
/* Valid {n}, {n,}, {n,m} at r->p: returns its length, or 0 when '{' is a literal. */
static int brace_len(const char *p, const char *e) {
  const char *q = p + 1;
  if (q >= e || *q < '0' || *q > '9') return 0;
  while (q < e && *q >= '0' && *q <= '9') q++;
  if (q < e && *q == ',') { q++; while (q < e && *q >= '0' && *q <= '9') q++; }
  if (q < e && *q == '}') return (int)(q - p + 1);
  return 0;
}
static Node *parse_disj(RP *r);

/* Parses one term (an atom with an optional quantifier, or an assertion). */
static Node *parse_term(RP *r) {
  int c = *r->p;
  if (c == '^') { r->p++; Node *n = new_node(N_BOL); n->multi = r->multi; return n; }
  if (c == '$') { r->p++; Node *n = new_node(N_EOL); n->multi = r->multi; return n; }
  if (c == '\\' && (r->p[1] == 'b' || r->p[1] == 'B')) {
    Node *n = new_node(N_WB); n->neg = r->p[1] == 'B'; r->p += 2; return n;
  }
  int capbefore = r->ncap;
  Node *atom;
  if (c == '(') {
    r->p++;
    if (*r->p == '?') {
      if (r->p[1] == ':') { r->p += 2; atom = new_node(N_GROUP); }
      else if (r->p[1] == '=' || r->p[1] == '!') { atom = new_node(N_LOOK); atom->neg = r->p[1] == '!'; r->p += 2; }
      else rx_fail("unsupported group", r->src);
    } else {
      atom = new_node(N_GROUP);
      atom->cap = ++r->ncap;
    }
    atom->child = parse_disj(r);
    if (*r->p != ')') rx_fail("missing )", r->src);
    r->p++;
    if (atom->kind == N_LOOK) return atom;
  } else if (c == '.') {
    r->p++; atom = new_node(N_DOT); atom->dotall = r->dotall;
  } else if (c == '[') {
    r->p++; atom = parse_class(r);
  } else if (c == '\\') {
    r->p++;
    int d = *r->p;
    if (d == 'd' || d == 'D' || d == 's' || d == 'S' || d == 'w' || d == 'W') {
      r->p++; atom = new_node(N_SET); class_escape(atom->set, d);
    } else if (d >= '1' && d <= '9') {
      int v = 0;
      while (*r->p >= '0' && *r->p <= '9') v = v * 10 + (*r->p++ - '0');
      atom = new_node(N_BREF); atom->cap = v; atom->icase = r->icase;
    } else {
      int ch = char_escape(r);
      atom = single_char(ch, r->icase);
    }
  } else {
    r->p++;
    atom = single_char(c, r->icase);
  }
  /* quantifier */
  int min = -2, max = -1;
  if (r->p < r->e) {
    int q = *r->p;
    if (q == '*') { min = 0; max = -1; r->p++; }
    else if (q == '+') { min = 1; max = -1; r->p++; }
    else if (q == '?') { min = 0; max = 1; r->p++; }
    else if (q == '{') {
      int len = brace_len(r->p, r->e);
      if (len) {
        const char *s = r->p + 1;
        min = 0;
        while (*s >= '0' && *s <= '9') min = min * 10 + (*s++ - '0');
        if (*s == ',') {
          s++;
          if (*s == '}') max = -1;
          else { max = 0; while (*s >= '0' && *s <= '9') max = max * 10 + (*s++ - '0'); }
        } else max = min;
        r->p += len;
      }
    }
  }
  if (min == -2) return atom;
  Node *qn = new_node(N_QUANT);
  qn->child = atom; qn->min = min; qn->max = max; qn->greedy = 1;
  if (r->p < r->e && *r->p == '?') { qn->greedy = 0; r->p++; }
  qn->cap_lo = capbefore + 1; qn->cap_hi = r->ncap;
  return qn;
}
static Node *parse_seq(RP *r) {
  Node *head = NULL, *tail = NULL;
  while (r->p < r->e && *r->p != '|' && *r->p != ')') {
    Node *t = parse_term(r);
    if (!head) head = t; else tail->next = t;
    tail = t;
  }
  return head;
}
static Node *parse_disj(RP *r) {
  Node *alts[512];
  int n = 0;
  for (;;) {
    if (n == 512) rx_fail("too many alternatives", r->src);
    alts[n++] = parse_seq(r);
    if (r->p < r->e && *r->p == '|') { r->p++; continue; }
    break;
  }
  if (n == 1) return alts[0];
  Node *a = new_node(N_ALT);
  a->alts = ar_alloc(n * sizeof(Node *));
  memcpy(a->alts, alts, n * sizeof(Node *));
  a->nalts = n;
  return a;
}
/* Set of possible first characters of a chain that cannot match empty. */
static int first_of(const Node *n, uint8_t *out) {
  if (!n) return 0;
  switch (n->kind) {
    case N_SET: memcpy(out, n->set, 32); return 1;
    case N_GROUP: return first_of(n->child, out);
    case N_ALT: {
      memset(out, 0, 32);
      for (int a = 0; a < n->nalts; a++) {
        uint8_t t[32];
        if (!first_of(n->alts[a], t)) return 0;
        set_or(out, t);
      }
      return 1;
    }
    case N_QUANT: return n->min >= 1 ? first_of(n->child, out) : 0;
    default: return 0;
  }
}
static Rx *rx_compile(const char *src, const char *flags) {
  RP r = {src, src + strlen(src), 0, 0, 0, 0, src};
  for (const char *f = flags; *f; f++) {
    if (*f == 'i') r.icase = 1;
    else if (*f == 'm') r.multi = 1;
    else if (*f == 's') r.dotall = 1;
  }
  r.p = src;
  Rx *rx = ar_alloc(sizeof(Rx));
  rx->root = parse_disj(&r);
  if (r.p != r.e) rx_fail("unbalanced )", src);
  rx->known = first_of(rx->root, rx->first);
  return rx;
}

/* Matcher state for one exec. */
static const char *M_s;
static int M_len, M_mend;
static int M_caps[2 * MAXCAP];

typedef struct K K;
enum { K_SEQ, K_GROUP, K_REP, K_LOOK, K_ACCEPT };
struct K { int kind; const Node *n; int a, b; const K *next; };

static int m_seq(const Node *n, int i, const K *k);

static int m_k(const K *k, int i) {
  switch (k->kind) {
    case K_SEQ: return m_seq(k->n, i, k->next);
    case K_GROUP: {
      int idx = 2 * k->n->cap, os = M_caps[idx], oe = M_caps[idx + 1];
      M_caps[idx] = k->a; M_caps[idx + 1] = i;
      if (m_k(k->next, i)) return 1;
      M_caps[idx] = os; M_caps[idx + 1] = oe;
      return 0;
    }
    case K_REP: {
      /* an iteration that matched empty is refused once the minimum is satisfied */
      if (i == k->b && k->a - 1 >= k->n->min) return 0;
      /* m_rep is defined below */
      extern int m_rep_entry(const Node *q, int count, int i, const K *after);
      return m_rep_entry(k->n, k->a, i, k->next);
    }
    case K_LOOK: return 1;
    case K_ACCEPT: M_mend = i; return 1;
  }
  return 0;
}

static int m_rep_child(const Node *q, int count, int i, const K *after) {
  K rk = {K_REP, q, count + 1, i, after};
  int saved[2 * MAXCAP];
  int lo = 2 * q->cap_lo, hi = 2 * q->cap_hi + 2;
  if (hi > 2 * MAXCAP) hi = 2 * MAXCAP;
  for (int x = lo; x < hi; x++) { saved[x] = M_caps[x]; M_caps[x] = -1; }
  if (m_seq(q->child, i, &rk)) return 1;
  for (int x = lo; x < hi; x++) M_caps[x] = saved[x];
  return 0;
}
int m_rep_entry(const Node *q, int count, int i, const K *after) {
  if (count < q->min) return m_rep_child(q, count, i, after);
  if (q->max >= 0 && count >= q->max) return m_k(after, i);
  if (q->greedy) {
    if (m_rep_child(q, count, i, after)) return 1;
    return m_k(after, i);
  }
  if (m_k(after, i)) return 1;
  return m_rep_child(q, count, i, after);
}
static int char_ok(const Node *c, int ch) {
  if (c->kind == N_DOT) return c->dotall || !is_lt(ch);
  return set_has(c->set, ch);
}
static int m_seq(const Node *n, int i, const K *k) {
  if (!n) return m_k(k, i);
  switch (n->kind) {
    case N_SET:
      if (i < M_len && set_has(n->set, (unsigned char)M_s[i])) return m_seq(n->next, i + 1, k);
      return 0;
    case N_DOT:
      if (i < M_len && char_ok(n, (unsigned char)M_s[i])) return m_seq(n->next, i + 1, k);
      return 0;
    case N_BOL:
      if (i == 0 || (n->multi && is_lt((unsigned char)M_s[i - 1]))) return m_seq(n->next, i, k);
      return 0;
    case N_EOL:
      if (i == M_len || (n->multi && is_lt((unsigned char)M_s[i]))) return m_seq(n->next, i, k);
      return 0;
    case N_WB: {
      int a = i > 0 && is_word((unsigned char)M_s[i - 1]);
      int b = i < M_len && is_word((unsigned char)M_s[i]);
      int at = a != b;
      if (n->neg ? !at : at) return m_seq(n->next, i, k);
      return 0;
    }
    case N_BREF: {
      int s0 = M_caps[2 * n->cap], e0 = M_caps[2 * n->cap + 1];
      if (s0 < 0 || e0 < 0) return m_seq(n->next, i, k);
      int L = e0 - s0;
      if (i + L > M_len) return 0;
      for (int x = 0; x < L; x++) {
        int p = (unsigned char)M_s[s0 + x], q = (unsigned char)M_s[i + x];
        if (n->icase ? lower(p) != lower(q) : p != q) return 0;
      }
      return m_seq(n->next, i + L, k);
    }
    case N_GROUP: {
      K sk = {K_SEQ, n->next, 0, 0, k};
      if (n->cap) {
        K gk = {K_GROUP, n, i, 0, &sk};
        return m_seq(n->child, i, &gk);
      }
      return m_seq(n->child, i, &sk);
    }
    case N_ALT:
      for (int a = 0; a < n->nalts; a++) {
        K sk = {K_SEQ, n->next, 0, 0, k};
        if (m_seq(n->alts[a], i, &sk)) return 1;
      }
      return 0;
    case N_LOOK: {
      K acc = {K_LOOK, NULL, 0, 0, NULL};
      int r = m_seq(n->child, i, &acc);
      if (n->neg ? r : !r) return 0;
      return m_seq(n->next, i, k);
    }
    case N_QUANT: {
      K sk = {K_SEQ, n->next, 0, 0, k};
      const Node *c = n->child;
      if ((c->kind == N_SET || c->kind == N_DOT) && !c->next) {
        /* single-character atom: count the run, then try continuations without recursion */
        int run = 0;
        while ((n->max < 0 || run < n->max) && i + run < M_len && char_ok(c, (unsigned char)M_s[i + run])) run++;
        if (run < n->min) return 0;
        if (n->greedy) {
          for (int x = run; x >= n->min; x--) if (m_k(&sk, i + x)) return 1;
        } else {
          for (int x = n->min; x <= run; x++) if (m_k(&sk, i + x)) return 1;
        }
        return 0;
      }
      return m_rep_entry(n, 0, i, &sk);
    }
  }
  return 0;
}

/* Leftmost match at or after from. caps[2g], caps[2g+1] are group g (-1 when unset). */
static int rx_exec(const Rx *rx, const char *s, int len, int from, int *caps) {
  if (from > len) return 0;
  M_s = s; M_len = len;
  for (int start = from; start <= len; start++) {
    if (rx->known) {
      if (start >= len || !set_has(rx->first, (unsigned char)s[start])) continue;
    }
    for (int x = 0; x < 2 * MAXCAP; x++) M_caps[x] = -1;
    M_caps[0] = start;
    K acc = {K_ACCEPT, NULL, 0, 0, NULL};
    if (m_seq(rx->root, start, &acc)) {
      M_caps[0] = start; M_caps[1] = M_mend;
      memcpy(caps, M_caps, sizeof M_caps);
      return 1;
    }
  }
  return 0;
}

/* ---- grammars: built per iteration from the generated tables ---- */
typedef struct Grammar Grammar;
typedef struct Pat { Rx *re; int lb, gr; Grammar *inside; int nal; const char *al[2]; } Pat;
typedef struct Ent { const char *key; int is_rest; Grammar *rest; int npats; Pat **pats; } Ent;
struct Grammar { int nents; Ent *ents; };

static Grammar *G_TABLE;
static Pat *P_TABLE;

static void build_grammars(void) {
  int np = (int)(sizeof PATS / sizeof PATS[0]);
  P_TABLE = ar_alloc(np * sizeof(Pat));
  for (int i = 0; i < np; i++) {
    Pat *p = &P_TABLE[i];
    p->re = rx_compile(PATS[i].src, PATS[i].flags);
    p->lb = PATS[i].lookbehind;
    p->gr = PATS[i].greedy;
    p->nal = PATS[i].naliases;
    for (int a = 0; a < p->nal; a++) p->al[a] = PATS[i].aliases[a];
  }
  for (int i = 0; i < np; i++) if (PATS[i].inside >= 0) P_TABLE[i].inside = NULL;
  int ng = N_GRAMS;
  G_TABLE = ar_alloc(ng * sizeof(Grammar));
  for (int g = 0; g < ng; g++) {
    int ne = GRAMS[g].nents, first = GRAMS[g].first;
    G_TABLE[g].nents = ne;
    G_TABLE[g].ents = ar_alloc((ne + 1) * sizeof(Ent));
    for (int e = 0; e < ne; e++) {
      Ent *E = &G_TABLE[g].ents[e];
      E->key = ENTS[first + e].key;
      E->is_rest = ENTS[first + e].rest >= 0;
      E->rest = E->is_rest ? &G_TABLE[ENTS[first + e].rest] : NULL;
      E->npats = ENTS[first + e].npats;
      E->pats = ar_alloc((E->npats + 1) * sizeof(Pat *));
      for (int j = 0; j < E->npats; j++) E->pats[j] = &P_TABLE[ENT_PATS[ENTS[first + e].first + j]];
    }
  }
  for (int i = 0; i < np; i++) if (PATS[i].inside >= 0) P_TABLE[i].inside = &G_TABLE[PATS[i].inside];
}
static Grammar *lang_grammar(const char *name) {
  for (int i = 0; i < N_LANGS; i++) if (strcmp(LANGS[i].name, name) == 0) return &G_TABLE[LANGS[i].gid];
  return NULL;
}

/* Prism.tokenize: first use merges the rest grammar into this grammar. */
static void merge_rest(Grammar *g) {
  int ri = -1;
  for (int e = 0; e < g->nents; e++) if (g->ents[e].is_rest) { ri = e; break; }
  if (ri < 0) return;
  Grammar *rg = g->ents[ri].rest;
  Ent *ents = ar_alloc((g->nents + rg->nents + 1) * sizeof(Ent));
  int n = 0;
  for (int e = 0; e < g->nents; e++) if (e != ri) ents[n++] = g->ents[e];
  for (int e = 0; e < rg->nents; e++) {
    Ent *E = &rg->ents[e];
    if (E->is_rest) { fprintf(stderr, "nested rest is not handled\n"); exit(5); }
    int found = -1;
    for (int x = 0; x < n; x++) if (strcmp(ents[x].key, E->key) == 0) { found = x; break; }
    if (found >= 0) ents[found] = *E; else ents[n++] = *E;
  }
  g->ents = ents;
  g->nents = n;
}

/* ---- tokens and the linked token list ---- */
typedef struct Tok Tok;
typedef struct Val { const char *s; int len; Tok *t; } Val; /* string piece, or token when t != NULL */
struct Tok {
  const char *type;
  int mlen;                    /* Token.length: the matched string's length */
  int is_str; const char *cs; int clen;   /* content as a string ... */
  Val *kids; int nkids;                    /* ... or as an array of values */
  const char *alias[4]; int nal;
};
typedef struct LNode { Val v; struct LNode *prev, *next; } LNode;
typedef struct { LNode *head, *tail; int length; } LList;

static void list_init(LList *L) {
  L->head = ar_alloc(sizeof(LNode));
  L->tail = ar_alloc(sizeof(LNode));
  L->head->next = L->tail; L->tail->prev = L->head;
  L->length = 0;
}
static LNode *add_after(LList *L, LNode *node, Val v) {
  LNode *next = node->next;
  LNode *nn = ar_alloc(sizeof(LNode));
  nn->v = v; nn->prev = node; nn->next = next;
  node->next = nn; next->prev = nn;
  L->length++;
  return nn;
}
static void remove_range(LList *L, LNode *node, int count) {
  LNode *next = node->next;
  int i = 0;
  while (i < count && next != L->tail) { next = next->next; i++; }
  node->next = next; next->prev = node;
  L->length -= i;
}
static int vlen(const LNode *n) { return n->v.t ? n->v.t->mlen : n->v.len; }

typedef struct { int cause_ent, cause_j, reach; } RM;
static Val *tokenize(const char *text, int tlen, Grammar *g, int *nout);

/* matchPattern: exec from pos; a lookbehind group (match[1]) is cut from the match. */
static int match_pattern(const Pat *p, int pos, const char *s, int len, int *index, int *mlen) {
  int caps[2 * MAXCAP];
  if (!rx_exec(p->re, s, len, pos, caps)) return 0;
  int idx = caps[0], ml = caps[1] - caps[0];
  if (p->lb && caps[2] >= 0 && caps[3] - caps[2] > 0) {
    int l = caps[3] - caps[2];
    idx += l; ml -= l;
  }
  *index = idx; *mlen = ml;
  return 1;
}

static void match_grammar(const char *text, int tlen, LList *list, Grammar *g, LNode *startNode, int startPos, RM *rematch) {
  for (int ei = 0; ei < g->nents; ei++) {
    for (int j = 0; j < g->ents[ei].npats; j++) {
      if (rematch && rematch->cause_ent == ei && rematch->cause_j == j) return;
      const Pat *pt = g->ents[ei].pats[j];
      const char *tokType = g->ents[ei].key;
      Grammar *inside = pt->inside;
      int greedy = pt->gr;
      LNode *cur = startNode->next;
      int pos = startPos;
      while (cur != list->tail && !(rematch && pos >= rematch->reach)) {
        if (list->length > tlen) return;
        if (!cur->v.t) {
          int removeCount = 1;
          const char *str; int slen;
          int from, mlen;
          if (greedy) {
            int mi, ml;
            if (!match_pattern(pt, pos, text, tlen, &mi, &ml) || mi >= tlen) break;
            from = mi;
            int to = mi + ml;
            int p = pos;
            for (p += vlen(cur); from >= p; ) { cur = cur->next; p += vlen(cur); }
            pos = p -= vlen(cur);
            if (cur->v.t) goto next_node;
            for (LNode *k = cur; k != list->tail && (p < to || !k->v.t); k = k->next) { removeCount++; p += vlen(k); }
            removeCount--;
            str = text + pos; slen = p - pos;
            from = mi - pos;
            mlen = ml;
          } else {
            str = cur->v.s; slen = cur->v.len;
            int mi, ml;
            if (!match_pattern(pt, 0, str, slen, &mi, &ml)) goto next_node;
            from = mi; mlen = ml;
          }
          {
            const char *matchStr = str + from;
            int before = from, after = slen - (from + mlen);
            int reach = pos + slen;
            if (rematch && reach > rematch->reach) rematch->reach = reach;
            LNode *removeFrom = cur->prev;
            if (before > 0) {
              Val bv = {str, before, NULL};
              removeFrom = add_after(list, removeFrom, bv);
              pos += before;
            }
            remove_range(list, removeFrom, removeCount);
            Tok *t = ar_alloc(sizeof(Tok));
            t->type = tokType;
            t->mlen = mlen;
            if (inside) {
              int nk;
              t->kids = tokenize(matchStr, mlen, inside, &nk);
              t->nkids = nk;
            } else {
              t->is_str = 1; t->cs = matchStr; t->clen = mlen;
            }
            if (pt->nal) { for (int a = 0; a < pt->nal && a < 4; a++) t->alias[t->nal++] = pt->al[a]; }
            Val tv = {NULL, 0, t};
            cur = add_after(list, removeFrom, tv);
            if (after > 0) {
              Val av = {str + from + mlen, after, NULL};
              add_after(list, cur, av);
            }
            if (removeCount > 1) {
              RM nested = {ei, j, reach};
              match_grammar(text, tlen, list, g, cur->prev, pos, &nested);
              if (rematch && nested.reach > rematch->reach) rematch->reach = nested.reach;
            }
          }
        }
      next_node:
        pos += vlen(cur);
        cur = cur->next;
      }
    }
  }
}

static Val *tokenize(const char *text, int tlen, Grammar *g, int *nout) {
  merge_rest(g);
  LList L;
  list_init(&L);
  Val v = {text, tlen, NULL};
  add_after(&L, L.head, v);
  match_grammar(text, tlen, &L, g, L.head, 0, NULL);
  int n = 0;
  for (LNode *x = L.head->next; x != L.tail; x = x->next) n++;
  Val *arr = ar_alloc((n + 1) * sizeof(Val));
  int k = 0;
  for (LNode *x = L.head->next; x != L.tail; x = x->next) arr[k++] = x->v;
  *nout = n;
  return arr;
}

/* ---- output buffers ---- */
typedef struct { char *d; size_t n, cap; } Buf;
static void buf_put(Buf *b, const char *s, size_t n) {
  if (b->n + n + 1 > b->cap) {
    size_t c = b->cap ? b->cap * 2 : 256;
    while (c < b->n + n + 1) c *= 2;
    b->d = realloc(b->d, c);
    b->cap = c;
  }
  memcpy(b->d + b->n, s, n);
  b->n += n;
  b->d[b->n] = 0;
}
static void buf_str(Buf *b, const char *s) { buf_put(b, s, strlen(s)); }
/* encode(): escapes & and < (no-break spaces are not in the ASCII samples). */
static void encode_put(Buf *b, const char *s, int n) {
  for (int i = 0; i < n; i++) {
    if (s[i] == '&') buf_str(b, "&amp;");
    else if (s[i] == '<') buf_str(b, "&lt;");
    else buf_put(b, s + i, 1);
  }
}

static void highlight_into(Buf *out, const char *text, int tlen, Grammar *g, const char *lang);
static void emit_val(Buf *out, const Val *v, const char *lang);

/* Prism's markdown after-tokenize hook: a fenced block's language becomes an alias. */
static void walk_tokens(Val *vals, int n) {
  for (int i = 0; i < n; i++) {
    Tok *t = vals[i].t;
    if (!t) continue;
    if (strcmp(t->type, "code") == 0) {
      if (t->is_str || t->nkids < 4) continue;
      Val *cl = &t->kids[1], *cb = &t->kids[3];
      if (!cl->t || !cb->t) continue;
      if (strcmp(cl->t->type, "code-language") != 0 || strcmp(cb->t->type, "code-block") != 0) continue;
      if (!cl->t->is_str) continue;
      /* lang = content.replace(/\b#/g,'sharp').replace(/\b\+\+/g,'pp') */
      Buf l1 = {0};
      for (int x = 0; x < cl->t->clen; x++) {
        int c = cl->t->cs[x];
        if (c == '#' && x > 0 && is_word((unsigned char)cl->t->cs[x - 1])) buf_str(&l1, "sharp");
        else if (c == '+' && x + 1 < cl->t->clen && cl->t->cs[x + 1] == '+' && x > 0 && is_word((unsigned char)cl->t->cs[x - 1])) { buf_str(&l1, "pp"); x++; }
        else buf_put(&l1, (const char *)&cl->t->cs[x], 1);
      }
      /* first match of [a-z][a-z0-9_-]* ignoring case, lowercased; empty when none */
      const char *ls = l1.d ? l1.d : "";
      size_t ln = l1.n, st = 0;
      while (st < ln && !((ls[st] >= 'a' && ls[st] <= 'z') || (ls[st] >= 'A' && ls[st] <= 'Z'))) st++;
      size_t en = st;
      if (st < ln) { en = st + 1; while (en < ln && (is_word((unsigned char)ls[en]) || ls[en] == '-')) en++; }
      Buf al = {0};
      buf_str(&al, "language-");
      for (size_t x = st; x < en; x++) { char c = (char)lower((unsigned char)ls[x]); buf_put(&al, &c, 1); }
      char *aliasStr = ar_alloc(al.n + 1);
      memcpy(aliasStr, al.d, al.n);
      if (cb->t->nal < 4) cb->t->alias[cb->t->nal++] = aliasStr;
      free(al.d);
      free(l1.d);
    } else if (!t->is_str) {
      walk_tokens(t->kids, t->nkids);
    }
  }
}

/* Prism.highlight: tokenize, after-tokenize hook, then encode and stringify. */
static void highlight_into(Buf *out, const char *text, int tlen, Grammar *g, const char *lang) {
  int n;
  Val *vals = tokenize(text, tlen, g, &n);
  if (strcmp(lang, "markdown") == 0 || strcmp(lang, "md") == 0) walk_tokens(vals, n);
  for (int i = 0; i < n; i++) emit_val(out, &vals[i], lang);
}

/* Replacement of /<tag pattern>/gi with "" (the code-block wrap strips tags first). */
static void strip_tags(Buf *out, const char *s, int n, const Rx *tag) {
  int caps[2 * MAXCAP];
  int pos = 0, last = 0;
  while (pos <= n && rx_exec(tag, s, n, pos, caps)) {
    buf_put(out, s + last, caps[0] - last);
    last = caps[1];
    pos = caps[1] > caps[0] ? caps[1] : caps[1] + 1;
  }
  buf_put(out, s + last, n - last);
}
static const Rx *TAG_RX;

static void utf8_put(Buf *b, long cp) {
  char c[4]; int k;
  if (cp < 0x80) { c[0] = (char)cp; buf_put(b, c, 1); return; }
  if (cp < 0x800) { c[0] = (char)(0xC0 | (cp >> 6)); c[1] = (char)(0x80 | (cp & 63)); k = 2; }
  else if (cp < 0x10000) { c[0] = (char)(0xE0 | (cp >> 12)); c[1] = (char)(0x80 | ((cp >> 6) & 63)); c[2] = (char)(0x80 | (cp & 63)); k = 3; }
  else { c[0] = (char)(0xF0 | (cp >> 18)); c[1] = (char)(0x80 | ((cp >> 12) & 63)); c[2] = (char)(0x80 | ((cp >> 6) & 63)); c[3] = (char)(0x80 | (cp & 63)); k = 4; }
  buf_put(b, c, k);
}
/* html.replace(/&(\w{1,8}|#x?[\da-f]{1,8});/gi, decode) with the four known names. */
static void decode_entities(Buf *out, const char *s, int n) {
  int i = 0;
  while (i < n) {
    if (s[i] != '&') { buf_put(out, s + i, 1); i++; continue; }
    int j = i + 1, ok = 0, vstart = -1, vlen_ = 0, hex = 0;
    if (j < n && s[j] == '#') {
      int k = j + 1;
      if (k < n && (s[k] == 'x' || s[k] == 'X')) { hex = 1; k++; }
      int st = k;
      while (k < n && k - st < 8 && hexdig((unsigned char)s[k]) >= 0) k++;
      if (k > st && k < n && s[k] == ';') { ok = 1; vstart = j + 1; vlen_ = k - (j + 1); hex = hex && 1; }
      else if (hex) {
        /* "#x" without digits: x is not optional here, so no match */
      }
      if (ok) {
        const char *code = s + vstart;
        long v = 0;
        if (code[0] == 'x' || code[0] == 'X') {
          for (int x = 1; x < vlen_; x++) v = v * 16 + hexdig((unsigned char)code[x]);
        } else {
          for (int x = 0; x < vlen_; x++) v = v * 10 + (code[x] - '0');
        }
        utf8_put(out, v);
        i = k + 1;
        continue;
      }
    } else {
      int k = j;
      while (k < n && k - j < 8 && is_word((unsigned char)s[k])) k++;
      if (k > j && k < n && s[k] == ';') {
        int L = k - j;
        char name[9];
        for (int x = 0; x < L; x++) name[x] = (char)lower((unsigned char)s[j + x]);
        name[L] = 0;
        const char *rep = NULL;
        if (!strcmp(name, "amp")) rep = "&";
        else if (!strcmp(name, "lt")) rep = "<";
        else if (!strcmp(name, "gt")) rep = ">";
        else if (!strcmp(name, "quot")) rep = "\"";
        if (rep) buf_str(out, rep);
        else buf_put(out, s + i, k + 1 - i);
        i = k + 1;
        continue;
      }
    }
    buf_put(out, s + i, 1);
    i++;
  }
}

/* Token.stringify with the wrap hooks: code-block (re-highlight) and entity (title). */
static void emit_token(Buf *out, const Tok *t, const char *lang) {
  Buf content = {0};
  if (t->is_str) encode_put(&content, t->cs, t->clen);
  else for (int i = 0; i < t->nkids; i++) emit_val(&content, &t->kids[i], lang);
  char classes[512];
  int cl = snprintf(classes, sizeof classes, "token %s", t->type);
  for (int a = 0; a < t->nal; a++) cl += snprintf(classes + cl, sizeof classes - cl, " %s", t->alias[a]);
  const char *title = NULL;
  Buf titleBuf = {0};
  if (strcmp(t->type, "code-block") == 0) {
    const char *codeLang = "";
    /* classes: "token", type, then aliases; the first /language-(.+)/ class wins */
    for (int a = 0; a < t->nal; a++) {
      if (strncmp(t->alias[a], "language-", 9) == 0 && t->alias[a][9]) { codeLang = t->alias[a] + 9; break; }
    }
    Grammar *gr = codeLang[0] ? lang_grammar(codeLang) : NULL;
    if (gr) {
      Buf stripped = {0}, decoded = {0};
      strip_tags(&stripped, content.d ? content.d : "", (int)content.n, TAG_RX);
      decode_entities(&decoded, stripped.d ? stripped.d : "", (int)stripped.n);
      Buf hl = {0};
      highlight_into(&hl, decoded.d ? decoded.d : "", (int)decoded.n, gr, codeLang);
      free(content.d);
      content = hl;
      free(stripped.d);
      free(decoded.d);
    }
  }
  if (strcmp(t->type, "entity") == 0) {
    /* env.content.replace(/&amp;/, "&"): first occurrence only */
    buf_put(&titleBuf, content.d ? content.d : "", content.n);
    char *p = titleBuf.d ? strstr(titleBuf.d, "&amp;") : NULL;
    if (p) { size_t at = p - titleBuf.d; memmove(p + 1, p + 5, titleBuf.n - at - 5 + 1); titleBuf.d[at] = '&'; titleBuf.n -= 4; }
    title = titleBuf.d ? titleBuf.d : "";
  }
  buf_str(out, "<span class=\"");
  buf_str(out, classes);
  buf_str(out, "\"");
  if (title) {
    buf_str(out, " title=\"");
    for (const char *q = title; *q; q++) { if (*q == '"') buf_str(out, "&quot;"); else buf_put(out, q, 1); }
    buf_str(out, "\"");
  }
  buf_str(out, ">");
  if (content.d) buf_put(out, content.d, content.n);
  buf_str(out, "</span>");
  free(content.d);
  free(titleBuf.d);
}
static void emit_val(Buf *out, const Val *v, const char *lang) {
  if (!v->t) encode_put(out, v->s, v->len);
  else emit_token(out, v->t, lang);
}

static uint32_t quick_hash(const char *s, int len) {
  uint32_t hash = 5381;
  int i = len;
  while (i > 0) {
    uint32_t c = i < len ? (unsigned char)s[i] : 0; /* charCodeAt past the end is NaN, |0 is 0 */
    hash = (hash * 33u) ^ c;
    i -= 919;
  }
  return hash;
}

/* ---- the benchmark ---- */
typedef struct { const char *lang, *file; int32_t hash; char *content; int len; } Sample;
static Sample SAMPLES[] = {
  {"cpp", "sample.cpp", -1086372285, NULL, 0},
  {"css", "sample.css", 1173668337, NULL, 0},
  {"markup", "sample.html", -270772291, NULL, 0},
  {"js", "sample.js", -838545229, NULL, 0},
  {"markdown", "sample.md", 5859883, NULL, 0},
  {"sql", "sample.sql", 5859941, NULL, 0},
  {"json", "sample.json", 5859883, NULL, 0},
  {"typescript", "sample.ts", 133251625, NULL, 0},
};
#define N_SAMPLES ((int)(sizeof SAMPLES / sizeof SAMPLES[0]))

static char *read_file(const char *dir, const char *name, int *len) {
  char path[4096];
  snprintf(path, sizeof path, "%s/prismjs/data/%s", dir, name);
  FILE *f = fopen(path, "rb");
  if (!f) { perror(path); exit(2); }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *b = malloc(n + 1);
  if (fread(b, 1, n, f) != (size_t)n) { fprintf(stderr, "read failed: %s\n", path); exit(2); }
  b[n] = 0;
  fclose(f);
  *len = (int)n;
  return b;
}

int main(int argc, char **argv) {
  const char *root = argc > 1 ? argv[1] : "../jetstream";
  int iterations = argc > 2 ? atoi(argv[2]) : 120;
  const char *dump_dir = argc > 3 ? argv[3] : NULL; /* debugging: write iteration-0 html */
  for (int i = 0; i < N_SAMPLES; i++) SAMPLES[i].content = read_file(root, SAMPLES[i].file, &SAMPLES[i].len);

  int ok = 1;
  double elapsed = 0;
  for (int it = 0; it < iterations && ok; it++) {
    double t0 = now_ms();
    ar_reset();
    build_grammars();
    {
      /* markup.tag is the pattern of the grammar "markup" entry "tag" */
      Grammar *mk = lang_grammar("markup");
      for (int e = 0; e < mk->nents; e++) if (strcmp(mk->ents[e].key, "tag") == 0) TAG_RX = mk->ents[e].pats[0]->re;
    }
    for (int i = 0; i < N_SAMPLES; i++) {
      Sample *s = &SAMPLES[i];
      Grammar *g = lang_grammar(s->lang);
      Buf html = {0};
      highlight_into(&html, s->content, s->len, g, s->lang);
      if (it == 0 && dump_dir) {
        char path[4096];
        snprintf(path, sizeof path, "%s/c.%s.html", dump_dir, s->lang);
        FILE *df = fopen(path, "wb");
        if (df) { fwrite(html.d ? html.d : "", 1, html.n, df); fclose(df); }
      }
      uint32_t h = quick_hash(html.d ? html.d : "", (int)html.n);
      if ((int32_t)h != s->hash || html.n == 0) {
        fprintf(stderr, "hash mismatch for %s: %d expected %d\n", s->lang, (int32_t)h, s->hash);
        ok = 0;
      }
      free(html.d);
    }
    elapsed += now_ms() - t0;
  }
  printf("prismjs-startup-es6 %d iterations %.3f ms\n", iterations, elapsed);
  printf("validation: %s (8 html hashes per iteration)\n", ok ? "ok" : "FAILED");
  return ok ? 0 : 1;
}
