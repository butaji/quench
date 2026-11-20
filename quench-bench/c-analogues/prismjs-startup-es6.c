// jetstream-row: prismjs-startup-es6
// jetstream-files: utils/StartupBenchmark.js prismjs/benchmark.js
// iterations: 120
// jetstream-driver: class=Async preload=BUNDLE=prismjs/dist/bundle.es6.min.js,SAMPLE_CPP=prismjs/data/sample.cpp,SAMPLE_CSS=prismjs/data/sample.css,SAMPLE_HTML=prismjs/data/sample.html,SAMPLE_JS=prismjs/data/sample.js,SAMPLE_JSON=prismjs/data/sample.json,SAMPLE_MD=prismjs/data/sample.md,SAMPLE_PY=prismjs/data/sample.py,SAMPLE_SQL=prismjs/data/sample.sql,SAMPLE_TS=prismjs/data/sample.ts
// c-sources: -lpcre2-8
/* C analogue of JetStream's prismjs-startup-es6 row.
 *
 * The JS row re-evaluates the Prism bundle each iteration (a fresh Prism.languages, so the
 * grammars are rebuilt, and rest-merges happen again) and then highlights eight samples.
 * This analogue does the same work inside the timed loop:
 *   1. build all grammar objects: create every grammar RegExp (each is translated to PCRE2
 *      syntax and JIT-compiled on its first exec, as V8 compiles a RegExp on first use), build the entry lists and
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
#include <ctype.h>
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

/* ---- regular expressions: JS grammar patterns translated to PCRE2-JIT ---- */
/* Each grammar pattern is translated from JS syntax to PCRE2 syntax with the JS meaning
 * (JS \s and \S sets, '.' without s excludes \n and \r, ^ and $ with m match at \n or \r,
 * $ without m only at the end, \b \w \d ASCII, a backreference to a group that did not
 * participate matches the empty string) and compiled with pcre2_compile and
 * pcre2_jit_compile. Prism's bundle is re-evaluated every iteration, so the patterns are
 * compiled every iteration too (rx_release_all frees the previous iteration's code).
 * The matcher works on bytes (8-bit PCRE2 library, no UTF): the grammars' character
 * classes are written with code units up to ￿, and the highlighted samples are ASCII,
 * so a class range is clipped to the bytes 0x00-0xFF, which the samples cannot tell apart. */

#define PCRE2_CODE_UNIT_WIDTH 8
#include <pcre2.h>

#define JS_SPACE_BYTES "\\t\\n\\x{0b}\\f\\r \\x{a0}"
#define NOT_JS_SPACE_BYTES "\\x{00}-\\x{08}\\x{0e}-\\x{1f}\\x{21}-\\x{9f}\\x{a1}-\\x{ff}"

static void rx_fail(const char *msg, const char *src);

typedef struct Rx {
  const char *src, *flags;   /* JS source and flags, kept until the first exec */
  pcre2_code *code;          /* compiled on first use, as V8 compiles a RegExp on first exec */
  pcre2_match_data *md;
  int ngroups;
} Rx;

static pcre2_match_context *rx_mctx;
static Rx **rx_all;
static size_t rx_n, rx_cap;

typedef struct { char *s; size_t n, cap; } TBuf;

static void tb_put(TBuf *b, const char *s, size_t n) {
  if (b->n + n + 1 > b->cap) {
    b->cap = (b->n + n + 1) * 2;
    b->s = realloc(b->s, b->cap);
    if (!b->s) rx_fail("out of memory", "");
  }
  memcpy(b->s + b->n, s, n);
  b->n += n;
  b->s[b->n] = '\0';
}

static void tb_str(TBuf *b, const char *s) { tb_put(b, s, strlen(s)); }

static int tb_hex(const char *src, size_t n, size_t *i, int digits) {
  int v = 0;
  for (int k = 0; k < digits; k++) {
    int c = *i < n ? (unsigned char)src[*i] : -1, h;
    if (c >= '0' && c <= '9') h = c - '0';
    else if (c >= 'a' && c <= 'f') h = c - 'a' + 10;
    else if (c >= 'A' && c <= 'F') h = c - 'A' + 10;
    else rx_fail("bad hex escape", src);
    v = v * 16 + h;
    (*i)++;
  }
  return v;
}

/* Writes the code unit v as a PCRE2 escape; units above 0xFF are clipped to 0xFF (see above). */
static void tb_unit(TBuf *b, int v) {
  char tmp[16];
  snprintf(tmp, sizeof tmp, "\\x{%x}", v > 0xff ? 0xff : v);
  tb_str(b, tmp);
}

/* Translates a JS pattern (flags i, m, s, g) into PCRE2 syntax for 8-bit non-UTF mode. */
static void js_to_pcre2(const char *src, size_t n, int multi, int dotall, TBuf *out) {
  size_t i = 0;
  int in_class = 0;
  while (i < n) {
    unsigned char c = (unsigned char)src[i];
    if (c == '\\') {
      unsigned char e;
      if (i + 1 >= n) rx_fail("trailing backslash", src);
      e = (unsigned char)src[i + 1];
      i += 2;
      switch (e) {
        case 's':
          tb_str(out, in_class ? JS_SPACE_BYTES : "[" JS_SPACE_BYTES "]");
          break;
        case 'S':
          if (in_class) tb_str(out, NOT_JS_SPACE_BYTES);
          else tb_str(out, "[^" JS_SPACE_BYTES "]");
          break;
        case 'b': tb_str(out, in_class ? "\\x{08}" : "\\b"); break;
        case 'B': tb_str(out, "\\B"); break;
        case 'd': case 'D': case 'w': case 'W':
        case 'n': case 'r': case 't': case 'f':
          tb_put(out, "\\", 1);
          tb_put(out, (const char *)&e, 1);
          break;
        case 'v': tb_str(out, "\\x{0b}"); break;
        case '0': tb_str(out, "\\x{0}"); break;
        case 'x': tb_unit(out, tb_hex(src, n, &i, 2)); break;
        case 'u': tb_unit(out, tb_hex(src, n, &i, 4)); break;
        case 'c':
          if (i >= n || !isalpha((unsigned char)src[i])) rx_fail("bad \\c escape", src);
          tb_unit(out, (unsigned char)src[i++] % 32);
          break;
        default:
          if (e >= '1' && e <= '9') {
            if (in_class) rx_fail("backreference in a class", src);
            tb_put(out, "\\", 1);
            tb_put(out, (const char *)&e, 1);
          } else if (isalpha(e)) {
            tb_put(out, (const char *)&e, 1); /* identity escape of a letter */
          } else {
            tb_put(out, "\\", 1);             /* identity escape of punctuation */
            tb_put(out, (const char *)&e, 1);
          }
          break;
      }
      continue;
    }
    if (in_class) {
      if (c == ']') {
        in_class = 0;
        tb_put(out, "]", 1);
      } else if (c == '[') {
        tb_str(out, "\\[");
      } else {
        tb_put(out, (const char *)&c, 1);
      }
      i++;
      continue;
    }
    switch (c) {
      case '[':
        in_class = 1;
        tb_put(out, "[", 1);
        i++;
        if (i < n && src[i] == '^') {
          tb_put(out, "^", 1);
          i++;
        }
        break;
      case '.':
        tb_str(out, dotall ? "[\\x{00}-\\x{ff}]" : "[^\\n\\r]");
        i++;
        break;
      case '^':
        tb_str(out, multi ? "(?:^|(?<=[\\n\\r]))" : "^");
        i++;
        break;
      case '$':
        tb_str(out, multi ? "(?=[\\n\\r]|\\z)" : "\\z");
        i++;
        break;
      default:
        tb_put(out, (const char *)&c, 1);
        i++;
        break;
    }
  }
}

/* Creates the RegExp object of one JS pattern for this evaluation of the bundle; the
 * pattern is translated and JIT-compiled on its first exec (rx_build). */
static Rx *rx_compile(const char *src, const char *flags) {
  Rx *rx = malloc(sizeof *rx);
  if (!rx) rx_fail("out of memory", src);
  rx->src = src;
  rx->flags = flags;
  rx->code = NULL;
  rx->md = NULL;
  rx->ngroups = 0;
  if (rx_n == rx_cap) {
    rx_cap = rx_cap ? rx_cap * 2 : 1024;
    rx_all = realloc(rx_all, rx_cap * sizeof *rx_all);
    if (!rx_all) rx_fail("out of memory", src);
  }
  rx_all[rx_n++] = rx;
  return rx;
}

static void rx_build(Rx *rx) {
  static pcre2_jit_stack *jit_stack;
  TBuf pat = {0, 0, 0};
  uint32_t opts = PCRE2_MATCH_UNSET_BACKREF;
  int multi = 0, dotall = 0, err = 0;
  PCRE2_SIZE eoff = 0;
  uint32_t ng = 0;
  for (const char *f = rx->flags; *f; f++) {
    if (*f == 'i') opts |= PCRE2_CASELESS;
    else if (*f == 'm') multi = 1;
    else if (*f == 's') dotall = 1;
    else if (*f != 'g') rx_fail("regexp flag not used by the grammars", rx->src);
  }
  if (!rx_mctx) {
    jit_stack = pcre2_jit_stack_create(32 * 1024, 1024 * 1024, NULL);
    rx_mctx = pcre2_match_context_create(NULL);
    if (!jit_stack || !rx_mctx) rx_fail("cannot create PCRE2 JIT stack", "");
    pcre2_jit_stack_assign(rx_mctx, NULL, jit_stack);
    pcre2_set_match_limit(rx_mctx, 0xffffffffu); /* JS has no match limit */
  }
  js_to_pcre2(rx->src, strlen(rx->src), multi, dotall, &pat);
  rx->code = pcre2_compile((PCRE2_SPTR)pat.s, pat.n, opts, &err, &eoff, NULL);
  if (!rx->code) {
    PCRE2_UCHAR msg[256];
    pcre2_get_error_message(err, msg, sizeof msg);
    fprintf(stderr, "prismjs: %s at %zu in /%s/\n", (const char *)msg, (size_t)eoff, pat.s);
    exit(1);
  }
  if (pcre2_jit_compile(rx->code, PCRE2_JIT_COMPLETE) < 0) rx_fail("pcre2_jit_compile failed", rx->src);
  rx->md = pcre2_match_data_create_from_pattern(rx->code, NULL);
  pcre2_pattern_info(rx->code, PCRE2_INFO_CAPTURECOUNT, &ng);
  rx->ngroups = (int)ng;
  free(pat.s);
}

/* Frees the compiled patterns of the previous evaluation of the bundle. */
static void rx_release_all(void) {
  for (size_t i = 0; i < rx_n; i++) {
    if (rx_all[i]->md) pcre2_match_data_free(rx_all[i]->md);
    if (rx_all[i]->code) pcre2_code_free(rx_all[i]->code);
    free(rx_all[i]);
  }
  rx_n = 0;
}

static void rx_fail(const char *msg, const char *src) {
  fprintf(stderr, "prismjs: %s: %s\n", msg, src);
  exit(1);
}

/* First match at or after from (RegExp.prototype.exec with lastIndex = from).
 * caps receives start and end for groups 0..MAXCAP-1 (-1 when unset). */
static int rx_exec(const Rx *rx, const char *s, int len, int from, int *caps) {
  PCRE2_SIZE *ov;
  int rc;
  if (from > len) return 0;
  if (!rx->code) rx_build((Rx *)rx);
  rc = pcre2_match(rx->code, (PCRE2_SPTR)s, (PCRE2_SIZE)len, (PCRE2_SIZE)from, 0, rx->md, rx_mctx);
  if (rc == PCRE2_ERROR_NOMATCH) return 0;
  if (rc < 0) rx_fail("pcre2_match failed", "");
  ov = pcre2_get_ovector_pointer(rx->md);
  for (int g = 0; g < MAXCAP; g++) {
    if (g <= rx->ngroups && g < rc && ov[2 * g] != PCRE2_UNSET) {
      caps[2 * g] = (int)ov[2 * g];
      caps[2 * g + 1] = (int)ov[2 * g + 1];
    } else {
      caps[2 * g] = caps[2 * g + 1] = -1;
    }
  }
  return 1;
}

/* Character helpers of the text side (word characters, lower case, hex digits). */
static int is_word(int c) { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_'; }
static int lower(int c) { return c >= 'A' && c <= 'Z' ? c + 32 : c; }
static int hexdig(int c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  return -1;
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
  const char *root = argc > 2 ? argv[2] : "../jetstream";
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  const char *dump_dir = argc > 3 ? argv[3] : NULL; /* debugging: write iteration-0 html */
  for (int i = 0; i < N_SAMPLES; i++) SAMPLES[i].content = read_file(root, SAMPLES[i].file, &SAMPLES[i].len);

  int ok = 1;
  double elapsed = 0;
  for (int it = 0; it < iterations && ok; it++) {
    double t0 = now_ms();
    ar_reset();
    rx_release_all();
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
