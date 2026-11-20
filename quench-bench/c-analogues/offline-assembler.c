// jetstream-row: OfflineAssembler
// jetstream-files: RexBench/OfflineAssembler/registers.js RexBench/OfflineAssembler/instructions.js RexBench/OfflineAssembler/ast.js RexBench/OfflineAssembler/parser.js RexBench/OfflineAssembler/file.js RexBench/OfflineAssembler/LowLevelInterpreter.js RexBench/OfflineAssembler/LowLevelInterpreter32_64.js RexBench/OfflineAssembler/LowLevelInterpreter64.js RexBench/OfflineAssembler/InitBytecodes.js RexBench/OfflineAssembler/expected.js RexBench/OfflineAssembler/benchmark.js
// iterations: 80
/* C analogue of JetStream's RexBench/OfflineAssembler: the offlineasm lexer, parser and
 * AST construction for LowLevelInterpreter.asm (including the InitBytecodes, 64 and 32_64
 * modules it includes), run once per iteration with the AST state reset first, as the JS
 * runIteration() does. After the timed loop the last AST is dumped and compared line by
 * line with expected.js, as the JS validate() does.
 *
 * Port notes:
 * - The JS isRegister() has no return statement, so it is always falsy; register names
 *   are therefore parsed as identifiers. The port keeps that behaviour (the dump is the same).
 * - Only parse and dump are ported: that is all the benchmark runs (value() is unused).
 * - Nodes come from an arena that is reset each iteration instead of a garbage collector.
 * - Error-context strings that the JS builds eagerly are not built (only used on failure).
 * - "//" comments are skipped like "#" comments (see the lexer).
 * Usage: offlineassembler [jetstream-root] [iterations]
 */
#include <ctype.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define DEFAULT_ITERATIONS 80
#define ARENA_BYTES ((size_t)96 << 20)
#define MAP_SIZE 16384u
#define MAX_NAMES 16

static const char *default_root = "/home/user/quench/quench-bench/jetstream";

static void die(const char *msg) {
  fprintf(stderr, "offlineassembler: %s\n", msg);
  exit(1);
}

/* ---------- arena ---------- */
static unsigned char *arena;
static size_t arena_used;

static void *aalloc(size_t n) {
  n = (n + 15) & ~(size_t)15;
  if (arena_used + n > ARENA_BYTES) die("arena exhausted");
  void *p = arena + arena_used;
  arena_used += n;
  return p;
}

static char *arena_strdup(const char *s) {
  size_t n = strlen(s) + 1;
  char *p = aalloc(n);
  memcpy(p, s, n);
  return p;
}

/* ---------- source files (in memory, like the JS File directory) ---------- */
typedef struct { const char *name; char *data; size_t len; } SourceFile;
static SourceFile sources[] = {
  {"LowLevelInterpreter.asm", NULL, 0},
  {"InitBytecodes.asm", NULL, 0},
  {"LowLevelInterpreter64.asm", NULL, 0},
  {"LowLevelInterpreter32_64.asm", NULL, 0},
};
#define NUM_SOURCES (sizeof sources / sizeof sources[0])

static void load_sources(const char *root) {
  for (size_t i = 0; i < NUM_SOURCES; i++) {
    char path[4096];
    snprintf(path, sizeof path, "%s/RexBench/OfflineAssembler/%s", root, sources[i].name);
    FILE *f = fopen(path, "rb");
    if (!f) { fprintf(stderr, "cannot open %s\n", path); exit(1); }
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    sources[i].data = malloc((size_t)n + 1);
    if (fread(sources[i].data, 1, (size_t)n, f) != (size_t)n) die("short read");
    sources[i].data[n] = '\0';
    sources[i].len = (size_t)n;
    fclose(f);
  }
}

static SourceFile *open_source(const char *name) {
  for (size_t i = 0; i < NUM_SOURCES; i++)
    if (strcmp(sources[i].name, name) == 0) return &sources[i];
  fprintf(stderr, "no such file: %s\n", name);
  exit(1);
}

/* ---------- tokens ---------- */
typedef struct {
  const char *s;     /* token text inside the source (not NUL-terminated) */
  int len;
  int is_num;        /* JS Number token (0x and octal literals); value in num */
  double num;
  int line;
} Token;

static const char *cur_file;

static int is_alpha(int c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
static int is_digit(int c) { return c >= '0' && c <= '9'; }
static int is_word(int c) { return is_alpha(c) || is_digit(c) || c == '_'; }

/* Number.parseInt(text, radix) for a short digit string; exact up to 2^64, then rounded. */
static double parse_radix(const char *s, size_t n, int radix) {
  char buf[64];
  if (n == 0 || n >= sizeof buf) die("numeric literal too long");
  memcpy(buf, s, n);
  buf[n] = '\0';
  if (radix == 16) {
    unsigned long long v = strtoull(buf, NULL, 16);
    return (double)v;
  }
  return (double)strtoull(buf, NULL, radix);
}

/* Number.parseInt(decimal digits): correctly rounded like the JS. */
static double parse_decimal(const Token *t) {
  char buf[64];
  if (t->len <= 0 || (size_t)t->len >= sizeof buf) die("numeric literal too long");
  memcpy(buf, t->s, (size_t)t->len);
  buf[t->len] = '\0';
  return strtod(buf, NULL);
}

static Token *lex(const char *src, size_t len, int *count_out) {
  Token *toks = aalloc((len + 2) * sizeof(Token));
  size_t n = 0, p = 0;
  int line = 1;
  while (p < len) {
    unsigned char c = (unsigned char)src[p];
    size_t start = p;
    int is_num = 0;
    double num = 0;
    if (c == '#' || (c == '/' && p + 1 < len && src[p + 1] == '/')) {
      /* Comments. The JS lexer's "//" annotation branch never yields an Annotation token
       * (its annotation variable is block-scoped and reset before the newline), so it is
       * a plain comment here too. */
      while (p < len && src[p] != '\n') p++;
      continue;
    }
    if (c == '\n') {
      p++;
      toks[n++] = (Token){src + start, 1, 0, 0, line};
      line++;
      continue;
    }
    if (is_alpha(c)) {                       /* [a-zA-Z][a-zA-Z0-9_.]* */
      p++;
      while (p < len && (is_word(src[p]) || src[p] == '.')) p++;
    } else if (c == '.') {                   /* \.[a-zA-Z0-9_]* */
      p++;
      while (p < len && is_word(src[p])) p++;
    } else if (c == '_') {                   /* _[a-zA-Z0-9_]* */
      p++;
      while (p < len && is_word(src[p])) p++;
    } else if (c == ' ' || c == '\t') {
      while (p < len && (src[p] == ' ' || src[p] == '\t')) p++;
      continue;
    } else if (c == '0' && p + 2 < len && src[p + 1] == 'x' && isxdigit((unsigned char)src[p + 2])) {
      p += 2;                                /* Number.parseInt(digits, 16) */
      while (p < len && isxdigit((unsigned char)src[p])) p++;
      num = parse_radix(src + start + 2, p - start - 2, 16);
      is_num = 1;
    } else if (c == '0' && p + 1 < len && src[p + 1] >= '0' && src[p + 1] <= '7') {
      p++;                                   /* 0([0-7]+) : Number.parseInt(digits, 8) */
      while (p < len && src[p] >= '0' && src[p] <= '7') p++;
      num = parse_radix(src + start + 1, p - start - 1, 8);
      is_num = 1;
    } else if (is_digit(c)) {                /* [0-9]+ (kept as text, like the JS) */
      while (p < len && is_digit(src[p])) p++;
    } else if (c == ':' && p + 1 < len && src[p + 1] == ':') {
      p += 2;
    } else if (c && strchr(":,()[]=+-~|&^*", c)) {
      p++;
    } else if (c == '"') {                   /* ^".*" : greedy to the last quote on the line */
      size_t q = p + 1, last = 0;
      int found = 0;
      while (q < len && src[q] != '\n') {
        if (src[q] == '"') { last = q; found = 1; }
        q++;
      }
      if (!found || last == p) die("lexer error: unterminated string literal");
      p = last + 1;
    } else {
      fprintf(stderr, "lexer error at %s:%d\n", cur_file, line);
      exit(1);
    }
    toks[n++] = (Token){src + start, (int)(p - start), is_num, num, line};
  }
  toks[n] = (Token){"", 0, 0, 0, line};     /* sentinel: reads past the end see an empty token */
  *count_out = (int)n;
  return toks;
}

static int tok_is(const Token *t, const char *lit) {
  size_t n = strlen(lit);
  return !t->is_num && (size_t)t->len == n && memcmp(t->s, lit, n) == 0;
}

static int tok_text_is_one_of_scales(const Token *t) {
  return !t->is_num && t->len == 1 && (t->s[0] == '1' || t->s[0] == '2' || t->s[0] == '4' || t->s[0] == '8');
}

static int is_identifier_text(const Token *t) {       /* /^[a-zA-Z]([a-zA-Z0-9_.]*)$/ */
  if (t->is_num || t->len == 0 || !is_alpha(t->s[0])) return 0;
  for (int i = 1; i < t->len; i++)
    if (!(is_word(t->s[i]) || t->s[i] == '.')) return 0;
  return 1;
}

static int is_label_text(const Token *t) {           /* /^_([a-zA-Z0-9_]*)$/ */
  if (t->is_num || t->len == 0 || t->s[0] != '_') return 0;
  for (int i = 1; i < t->len; i++)
    if (!is_word(t->s[i])) return 0;
  return 1;
}

static int is_local_label_text(const Token *t) {     /* /^\.([a-zA-Z0-9_]*)$/ */
  if (t->is_num || t->len == 0 || t->s[0] != '.') return 0;
  for (int i = 1; i < t->len; i++)
    if (!is_word(t->s[i])) return 0;
  return 1;
}

/* The instruction table is generated from instructions.js (sorted, unique). */
static const char *const instruction_names[] = {
    "addd",
    "addi",
    "addis",
    "addp",
    "addps",
    "addq",
    "andi",
    "andp",
    "andq",
    "baddinz",
    "baddio",
    "baddis",
    "baddiz",
    "baddpnz",
    "baddpo",
    "baddps",
    "baddpz",
    "baddqnz",
    "baddqo",
    "baddqs",
    "baddqz",
    "bba",
    "bbaeq",
    "bbb",
    "bbbeq",
    "bbeq",
    "bbgt",
    "bbgteq",
    "bblt",
    "bblteq",
    "bbneq",
    "bcd2i",
    "bdeq",
    "bdequn",
    "bdgt",
    "bdgteq",
    "bdgtequn",
    "bdgtun",
    "bdlt",
    "bdlteq",
    "bdltequn",
    "bdltun",
    "bdneq",
    "bdnequn",
    "bia",
    "biaeq",
    "bib",
    "bibeq",
    "bieq",
    "bigt",
    "bigteq",
    "bilt",
    "bilteq",
    "bineq",
    "bmulinz",
    "bmulio",
    "bmulis",
    "bmuliz",
    "bnz",
    "bo",
    "borinz",
    "borio",
    "boris",
    "boriz",
    "bpa",
    "bpaeq",
    "bpb",
    "bpbeq",
    "bpeq",
    "bpgt",
    "bpgteq",
    "bplt",
    "bplteq",
    "bpneq",
    "bqa",
    "bqaeq",
    "bqb",
    "bqbeq",
    "bqeq",
    "bqgt",
    "bqgteq",
    "bqlt",
    "bqlteq",
    "bqneq",
    "break",
    "bs",
    "bsubinz",
    "bsubio",
    "bsubis",
    "bsubiz",
    "btbnz",
    "btbs",
    "btbz",
    "btd2i",
    "btinz",
    "btis",
    "btiz",
    "btpnz",
    "btps",
    "btpz",
    "btqnz",
    "btqs",
    "btqz",
    "bz",
    "call",
    "cba",
    "cbaeq",
    "cbb",
    "cbbeq",
    "cbeq",
    "cbgt",
    "cbgteq",
    "cblt",
    "cblteq",
    "cbneq",
    "cdqi",
    "ci2d",
    "cia",
    "ciaeq",
    "cib",
    "cibeq",
    "cieq",
    "cigt",
    "cigteq",
    "cilt",
    "cilteq",
    "cineq",
    "cloopCallJSFunction",
    "cloopCallNative",
    "cloopCallSlowPath",
    "cloopCallSlowPathVoid",
    "cloopCrash",
    "cloopDo",
    "clrbp",
    "cpa",
    "cpaeq",
    "cpb",
    "cpbeq",
    "cpeq",
    "cpgt",
    "cpgteq",
    "cplt",
    "cplteq",
    "cpneq",
    "cqa",
    "cqaeq",
    "cqb",
    "cqbeq",
    "cqeq",
    "cqgt",
    "cqgteq",
    "cqlt",
    "cqlteq",
    "cqneq",
    "divd",
    "emit",
    "fd2ii",
    "fd2q",
    "fii2d",
    "fq2d",
    "idivi",
    "jmp",
    "la",
    "leai",
    "leap",
    "loadb",
    "loadbs",
    "loadd",
    "loadh",
    "loadhs",
    "loadi",
    "loadis",
    "loadp",
    "loadq",
    "lshifti",
    "lshiftp",
    "lshiftq",
    "memfence",
    "movdz",
    "move",
    "moved",
    "movn",
    "movz",
    "muld",
    "muli",
    "mulp",
    "mulq",
    "mvlbl",
    "negi",
    "negp",
    "negq",
    "nop",
    "nopFixCortexA53Err835769",
    "noti",
    "ori",
    "oris",
    "orp",
    "orq",
    "pcrtoaddr",
    "peek",
    "pichdr",
    "poke",
    "pop",
    "push",
    "ret",
    "rshifti",
    "rshiftp",
    "rshiftq",
    "setcallreg",
    "slt",
    "sltu",
    "smulli",
    "sqrtd",
    "storeb",
    "stored",
    "storei",
    "storep",
    "storeq",
    "subd",
    "subi",
    "subis",
    "subp",
    "subq",
    "sxi2q",
    "tbnz",
    "tbs",
    "tbz",
    "td2i",
    "tinz",
    "tis",
    "tiz",
    "tpnz",
    "tps",
    "tpz",
    "tqnz",
    "tqs",
    "tqz",
    "urshifti",
    "urshiftp",
    "urshiftq",
    "xori",
    "xorp",
    "xorq",
    "zxi2q",
};
#define NUM_INSTRUCTIONS 244

static int is_instruction(const Token *t) {
  if (t->is_num) return 0;
  int lo = 0, hi = NUM_INSTRUCTIONS - 1;
  while (lo <= hi) {
    int mid = (lo + hi) / 2;
    const char *s = instruction_names[mid];
    size_t sl = strlen(s);
    size_t m = (size_t)t->len < sl ? (size_t)t->len : sl;
    int c = memcmp(t->s, s, m);
    if (c == 0) c = ((size_t)t->len > sl) - ((size_t)t->len < sl);
    if (c == 0) return 1;
    if (c < 0) hi = mid - 1; else lo = mid + 1;
  }
  return 0;
}

static int is_keyword(const Token *t) {
  static const char *const kw[] = {"true", "false", "if", "then", "else", "elsif", "end", "and", "or",
                                   "not", "global", "macro", "const", "constexpr", "sizeof", "error",
                                   "include"};
  if (t->is_num) return 0;
  for (size_t i = 0; i < sizeof kw / sizeof kw[0]; i++)
    if (tok_is(t, kw[i])) return 1;
  return is_instruction(t);     /* isRegister() is always falsy in the JS */
}

static int is_identifier(const Token *t) { return is_identifier_text(t) && !is_keyword(t); }
static int is_variable(const Token *t) { return is_identifier(t); }
static int is_label(const Token *t) { return is_label_text(t); }
static int is_local_label(const Token *t) { return is_local_label_text(t); }
static int is_integer(const Token *t) { return t->is_num || (t->len > 0 && is_digit(t->s[0])); }
static int is_string(const Token *t) { return !t->is_num && t->len > 0 && t->s[0] == '"'; }

/* ---------- AST ---------- */
enum {
  K_IMM, K_ADD, K_SUB, K_MUL, K_NEG, K_OR, K_AND, K_XOR, K_BITNOT, K_STRLIT, K_VAR,
  K_ADDRESS, K_BASEINDEX, K_ABSADDR, K_INSTR, K_ERROR, K_CONSTEXPR, K_CONSTDECL,
  K_LABEL, K_LOCALLABEL, K_LABELREF, K_LOCALLABELREF, K_STRUCTOFF, K_SIZEOF, K_SEQ,
  K_TRUE, K_FALSE, K_SETTING, K_PAND, K_POR, K_PNOT, K_SKIP, K_IFTE, K_MACRO, K_MACROCALL
};

typedef struct Node Node;
typedef struct Cell { Node *node; struct Cell *next; } Cell;
typedef struct { Cell *head, *tail; } List;

struct Node {
  int kind;
  int scale;
  double ival;
  const char *s1, *s2;
  Node *a, *b, *c;
  List list, list2;
};

static Node *g_true, *g_false;      /* True.instance / False.instance */

static Node *mk(int kind) {
  Node *n = aalloc(sizeof *n);
  memset(n, 0, sizeof *n);
  n->kind = kind;
  return n;
}

static void lpush(List *l, Node *n) {
  Cell *c = aalloc(sizeof *c);
  c->node = n;
  c->next = NULL;
  if (l->tail) l->tail->next = c; else l->head = c;
  l->tail = c;
}

static Node *mk1(int kind, Node *a) { Node *n = mk(kind); n->a = a; return n; }
static Node *mk2(int kind, Node *a, Node *b) { Node *n = mk(kind); n->a = a; n->b = b; return n; }

static Node *mk_imm(double v) { Node *n = mk(K_IMM); n->ival = v; return n; }

static void num_text(double v, char *out);

/* Mapping caches (Variable.forName, Label.forName, Setting.forName, ...), cleared by resetAST(). */
typedef struct { Node *val; uint32_t hash; } MapEnt;
static MapEnt map[MAP_SIZE];
static size_t map_count;

static uint32_t fnv(uint32_t h, const char *s) {
  for (; *s; s++) h = (h ^ (unsigned char)*s) * 16777619u;
  return h;
}

static Node *intern(int kind, const char *s1, const char *s2) {
  uint32_t h = fnv(2166136261u ^ (uint32_t)kind, s1);
  if (s2) h = fnv(h ^ 0x2au, s2);
  size_t i = h & (MAP_SIZE - 1);
  while (map[i].val) {
    Node *v = map[i].val;
    if (map[i].hash == h && v->kind == kind && strcmp(v->s1, s1) == 0 &&
        (s2 ? (v->s2 && strcmp(v->s2, s2) == 0) : v->s2 == NULL))
      return v;
    i = (i + 1) & (MAP_SIZE - 1);
  }
  if (++map_count > MAP_SIZE * 3 / 4) die("mapping table full");
  Node *n = mk(kind);
  n->s1 = arena_strdup(s1);
  if (s2) n->s2 = arena_strdup(s2);
  map[i].val = n;
  map[i].hash = h;
  return n;
}

static void reset_ast(void) {          /* resetAST() plus the arena reset that replaces GC */
  memset(map, 0, sizeof map);
  map_count = 0;
  arena_used = 0;
  g_true = mk(K_TRUE);
  g_false = mk(K_FALSE);
}

/* Name buffers: a token copied to a NUL-terminated string (valid until the next call). */
static char namebuf[4096], joinbuf[4096];

static const char *tok_cstr(const Token *t, char *buf) {
  if (t->is_num) {
    num_text(t->num, buf);
    return buf;
  }
  memcpy(buf, t->s, (size_t)t->len);
  buf[t->len] = '\0';
  return buf;
}

static void tok_append(char *buf, size_t *pos, const Token *t) {
  if (t->is_num) { char tmp[64]; num_text(t->num, tmp); size_t l = strlen(tmp); memcpy(buf + *pos, tmp, l); *pos += l; }
  else { memcpy(buf + *pos, t->s, (size_t)t->len); *pos += (size_t)t->len; }
  buf[*pos] = '\0';
}

/* ---------- parser ---------- */
typedef struct { Token *tok; int n; int idx; } Parser;

static Token *T(Parser *P) {
  if (P->idx > P->n) die("parse error at end of file");
  return &P->tok[P->idx];
}

static void parse_error(Parser *P) {
  Token *t = T(P);
  fprintf(stderr, "Parse error at %s:%d\n", cur_file, t->line);
  exit(1);
}

static void consume_re_then_or_nl(Parser *P) {
  Token *t = T(P);
  if (!(tok_is(t, "then") || tok_is(t, "\n"))) parse_error(P);
  P->idx++;
}

static void skip_newline(Parser *P) {
  while (tok_is(T(P), "\n")) P->idx++;
}

static Node *parse_predicate(Parser *P);

static Node *parse_predicate_atom(Parser *P) {
  Token *t = T(P);
  if (tok_is(t, "not")) {
    P->idx++;
    return mk1(K_PNOT, parse_predicate_atom(P));
  }
  if (tok_is(t, "(")) {
    P->idx++;
    skip_newline(P);
    Node *r = parse_predicate(P);
    if (!tok_is(T(P), ")")) parse_error(P);
    P->idx++;
    return r;
  }
  if (tok_is(t, "true")) { P->idx++; return g_true; }
  if (tok_is(t, "false")) { P->idx++; return g_false; }
  if (is_identifier(t)) {
    Node *r = intern(K_SETTING, tok_cstr(t, namebuf), NULL);
    P->idx++;
    return r;
  }
  parse_error(P);
  return NULL;
}

static Node *parse_predicate_and(Parser *P) {
  Node *result = parse_predicate_atom(P);
  while (tok_is(T(P), "and")) {
    P->idx++;
    skip_newline(P);
    Node *right = parse_predicate_atom(P);
    result = mk2(K_PAND, result, right);
  }
  return result;
}

static Node *parse_predicate(Parser *P) {
  Node *result = parse_predicate_and(P);
  while (tok_is(T(P), "or")) {
    P->idx++;
    skip_newline(P);
    Node *right = parse_predicate_and(P);
    result = mk2(K_POR, result, right);
  }
  return result;
}

static Node *parse_variable(Parser *P) {
  Node *result;
  Token *t = T(P);
  if (is_identifier(t)) result = intern(K_VAR, tok_cstr(t, namebuf), NULL);
  else { parse_error(P); return NULL; }
  P->idx++;
  return result;
}

static Node *parse_address(Parser *P, Node *offset) {
  if (!tok_is(T(P), "[")) parse_error(P);
  Node *result;
  P->idx++;
  if (tok_is(T(P), "]")) {
    P->idx++;
    return mk1(K_ABSADDR, offset);
  }
  Node *a = parse_variable(P);
  if (tok_is(T(P), "]")) {
    result = mk2(K_ADDRESS, a, offset);
  } else {
    if (!tok_is(T(P), ",")) parse_error(P);
    P->idx++;
    Node *b = parse_variable(P);
    if (tok_is(T(P), "]")) {
      result = mk(K_BASEINDEX);
      result->a = a; result->b = b; result->c = offset; result->scale = 1;
    } else {
      if (!tok_is(T(P), ",")) parse_error(P);
      P->idx++;
      if (!tok_text_is_one_of_scales(T(P))) parse_error(P);
      int scale = T(P)->s[0] - '0';
      P->idx++;
      if (!tok_is(T(P), "]")) parse_error(P);
      result = mk(K_BASEINDEX);
      result->a = a; result->b = b; result->c = offset; result->scale = scale;
    }
  }
  P->idx++;
  return result;
}

/* parseColonColon: fills names with token pointers, returns the count. */
static int parse_colon_colon(Parser *P, const Token **names) {
  skip_newline(P);
  if (!is_identifier(T(P))) parse_error(P);
  int n = 0;
  names[n++] = T(P);
  P->idx++;
  while (tok_is(T(P), "::")) {
    P->idx++;
    if (!is_identifier(T(P))) parse_error(P);
    if (n == MAX_NAMES) die("too many names");
    names[n++] = T(P);
    P->idx++;
  }
  return n;
}

/* parseTextInParens joined with "" (the only use). Writes into buf. */
static void parse_text_in_parens(Parser *P, char *buf) {
  skip_newline(P);
  if (!tok_is(T(P), "(")) parse_error(P);
  P->idx++;
  if (tok_is(T(P), ")")) parse_error(P);
  int enclosed = 0;
  size_t pos = 0;
  buf[0] = '\0';
  while (!tok_is(T(P), ")") || enclosed > 0) {
    if (P->idx >= P->n) parse_error(P);
    Token *t = T(P);
    if (tok_is(t, "(")) enclosed++;
    else if (tok_is(t, ")")) enclosed--;
    tok_append(buf, &pos, t);
    P->idx++;
  }
  P->idx++;
}

static Node *parse_expression(Parser *P);

static Node *parse_expression_atom(Parser *P) {
  skip_newline(P);
  Token *t = T(P);
  if (tok_is(t, "-")) {
    P->idx++;
    return mk1(K_NEG, parse_expression_atom(P));
  }
  if (tok_is(t, "~")) {
    P->idx++;
    return mk1(K_BITNOT, parse_expression_atom(P));
  }
  if (tok_is(t, "(")) {
    P->idx++;
    Node *r = parse_expression(P);
    if (!tok_is(T(P), ")")) parse_error(P);
    P->idx++;
    return r;
  }
  if (is_integer(t)) {
    double v = t->is_num ? t->num : parse_decimal(t);
    P->idx++;
    return mk_imm(v);
  }
  if (is_string(t)) {
    Node *r = mk(K_STRLIT);
    /* value is the text between the quotes */
    char *s = aalloc((size_t)t->len);
    memcpy(s, t->s + 1, (size_t)(t->len - 2));
    s[t->len - 2] = '\0';
    r->s1 = s;
    P->idx++;
    return r;
  }
  if (is_identifier(t)) {
    const Token *names[MAX_NAMES];
    int n = parse_colon_colon(P, names);
    if (n > 1) {
      size_t pos = 0;
      joinbuf[0] = '\0';
      for (int i = 0; i < n - 1; i++) {
        if (i) { memcpy(joinbuf + pos, "::", 2); pos += 2; }
        tok_append(joinbuf, &pos, names[i]);
      }
      const char *st = tok_cstr(names[n - 1], namebuf);
      return intern(K_STRUCTOFF, joinbuf, st);
    }
    return intern(K_VAR, tok_cstr(names[0], namebuf), NULL);
  }
  if (tok_is(t, "sizeof")) {
    P->idx++;
    const Token *names[MAX_NAMES];
    int n = parse_colon_colon(P, names);
    size_t pos = 0;
    joinbuf[0] = '\0';
    for (int i = 0; i < n; i++) {
      if (i) { memcpy(joinbuf + pos, "::", 2); pos += 2; }
      tok_append(joinbuf, &pos, names[i]);
    }
    return intern(K_SIZEOF, joinbuf, NULL);
  }
  if (tok_is(t, "constexpr")) {
    P->idx++;
    skip_newline(P);
    if (tok_is(T(P), "(")) {
      parse_text_in_parens(P, joinbuf);
    } else {
      const Token *names[MAX_NAMES];
      int n = parse_colon_colon(P, names);
      size_t pos = 0;
      joinbuf[0] = '\0';
      for (int i = 0; i < n; i++) {
        if (i) { memcpy(joinbuf + pos, "::", 2); pos += 2; }
        tok_append(joinbuf, &pos, names[i]);
      }
    }
    return intern(K_CONSTEXPR, joinbuf, NULL);
  }
  if (is_label(t)) {
    Node *lab = intern(K_LABEL, tok_cstr(t, namebuf), NULL);
    Node *r = mk1(K_LABELREF, lab);
    P->idx++;
    return r;
  }
  if (is_local_label(t)) {
    Node *lab = intern(K_LOCALLABEL, tok_cstr(t, namebuf), NULL);
    Node *r = mk1(K_LOCALLABELREF, lab);
    P->idx++;
    return r;
  }
  parse_error(P);
  return NULL;
}

static Node *parse_expression_mul(Parser *P) {
  skip_newline(P);
  Node *result = parse_expression_atom(P);
  while (tok_is(T(P), "*")) {
    P->idx++;
    result = mk2(K_MUL, result, parse_expression_atom(P));
  }
  return result;
}

static int could_be_expression(const Token *t) {
  return tok_is(t, "-") || tok_is(t, "~") || tok_is(t, "constexpr") || tok_is(t, "sizeof") ||
         is_integer(t) || is_string(t) || is_variable(t) || tok_is(t, "(");
}

static Node *parse_expression_add(Parser *P) {
  skip_newline(P);
  Node *result = parse_expression_mul(P);
  for (;;) {
    if (tok_is(T(P), "+")) {
      P->idx++;
      result = mk2(K_ADD, result, parse_expression_mul(P));
    } else if (tok_is(T(P), "-")) {
      P->idx++;
      result = mk2(K_SUB, result, parse_expression_mul(P));
    } else break;
  }
  return result;
}

static Node *parse_expression_and(Parser *P) {
  skip_newline(P);
  Node *result = parse_expression_add(P);
  while (tok_is(T(P), "&")) {
    P->idx++;
    result = mk2(K_AND, result, parse_expression_add(P));
  }
  return result;
}

static Node *parse_expression(Parser *P) {
  skip_newline(P);
  Node *result = parse_expression_and(P);
  for (;;) {
    if (tok_is(T(P), "|")) {
      P->idx++;
      result = mk2(K_OR, result, parse_expression_and(P));
    } else if (tok_is(T(P), "^")) {
      P->idx++;
      result = mk2(K_XOR, result, parse_expression_and(P));
    } else break;
  }
  return result;
}

static Node *parse_operand(Parser *P) {
  skip_newline(P);
  Token *t = T(P);
  if (could_be_expression(t)) {
    Node *expr = parse_expression(P);
    if (tok_is(T(P), "[")) return parse_address(P, expr);
    return expr;
  }
  if (tok_is(t, "[")) return parse_address(P, mk_imm(0));
  if (is_label(t)) {
    Node *lab = intern(K_LABEL, tok_cstr(t, namebuf), NULL);
    P->idx++;
    return mk1(K_LABELREF, lab);
  }
  if (is_local_label(t)) {
    Node *lab = intern(K_LOCALLABEL, tok_cstr(t, namebuf), NULL);
    P->idx++;
    return mk1(K_LOCALLABELREF, lab);
  }
  parse_error(P);
  return NULL;
}

static void parse_macro_variables(Parser *P, Node *mac) {
  skip_newline(P);
  if (!tok_is(T(P), "(")) parse_error(P);
  P->idx++;
  for (;;) {
    skip_newline(P);
    Token *t = T(P);
    if (tok_is(t, ")")) {
      P->idx++;
      break;
    } else if (is_identifier(t)) {
      lpush(&mac->list2, intern(K_VAR, tok_cstr(t, namebuf), NULL));
      P->idx++;
      skip_newline(P);
      if (tok_is(T(P), ")")) {
        P->idx++;
        break;
      } else if (tok_is(T(P), ",")) {
        P->idx++;
      } else parse_error(P);
    } else parse_error(P);
  }
}

/* final: 0 = none (end of file), 1 = else|end|elsif, 2 = end */
enum { FIN_NONE, FIN_ELSE_END_ELSIF, FIN_END };

static int is_final(const Token *t, int fin) {
  if (fin == FIN_NONE || t->is_num) return 0;
  if (fin == FIN_END) return tok_is(t, "end");
  return tok_is(t, "else") || tok_is(t, "end") || tok_is(t, "elsif");
}

static Node *parse_sequence(Parser *P, int fin);

static Node *parse_file(const char *name);

static Node *parse_sequence(Parser *P, int fin) {
  Node *seq = mk(K_SEQ);
  for (;;) {
    if (P->idx == P->n && fin == FIN_NONE) break;
    Token *t = T(P);
    if (is_final(t, fin)) break;
    if (tok_is(t, "\n")) {
      P->idx++;
    } else if (tok_is(t, "const")) {
      P->idx++;
      if (!is_variable(T(P))) parse_error(P);
      Node *var = intern(K_VAR, tok_cstr(T(P), namebuf), NULL);
      P->idx++;
      if (!tok_is(T(P), "=")) parse_error(P);
      P->idx++;
      Node *value = parse_operand(P);
      lpush(&seq->list, mk2(K_CONSTDECL, var, value));
    } else if (tok_is(t, "error")) {
      lpush(&seq->list, mk(K_ERROR));
      P->idx++;
    } else if (tok_is(t, "if")) {
      P->idx++;
      skip_newline(P);
      Node *predicate = parse_predicate(P);
      consume_re_then_or_nl(P);
      skip_newline(P);
      Node *ite = mk(K_IFTE);
      ite->a = predicate;
      ite->b = parse_sequence(P, FIN_ELSE_END_ELSIF);
      ite->c = mk(K_SKIP);
      lpush(&seq->list, ite);
      while (tok_is(T(P), "elsif")) {
        P->idx++;
        skip_newline(P);
        predicate = parse_predicate(P);
        consume_re_then_or_nl(P);
        skip_newline(P);
        Node *elseCase = mk(K_IFTE);
        elseCase->a = predicate;
        elseCase->b = parse_sequence(P, FIN_ELSE_END_ELSIF);
        elseCase->c = mk(K_SKIP);
        ite->c = elseCase;
        ite = elseCase;
      }
      if (tok_is(T(P), "else")) {
        P->idx++;
        ite->c = parse_sequence(P, FIN_END);
        P->idx++;
      } else {
        if (!tok_is(T(P), "end")) parse_error(P);
        P->idx++;
      }
    } else if (tok_is(t, "macro")) {
      P->idx++;
      skip_newline(P);
      if (!is_identifier(T(P))) parse_error(P);
      Node *mac = mk(K_MACRO);
      mac->s1 = arena_strdup(tok_cstr(T(P), namebuf));
      P->idx++;
      parse_macro_variables(P, mac);
      mac->a = parse_sequence(P, FIN_END);
      P->idx++;
      lpush(&seq->list, mac);
    } else if (tok_is(t, "global")) {
      P->idx++;
      skip_newline(P);
      if (!is_label(T(P))) parse_error(P);
      intern(K_LABEL, tok_cstr(T(P), namebuf), NULL);   /* Label.setAsGlobal: no output */
      P->idx++;
    } else if (is_instruction(t)) {
      const char *name = arena_strdup(tok_cstr(t, namebuf));
      P->idx++;
      Token *nt = T(P);
      if (is_final(nt, fin)) {
        lpush(&seq->list, mk(K_INSTR));
        seq->list.tail->node->s1 = name;
        break;
      } else if (tok_is(nt, "\n")) {
        Node *ins = mk(K_INSTR);
        ins->s1 = name;
        lpush(&seq->list, ins);
        P->idx++;
      } else {
        Node *ins = mk(K_INSTR);
        ins->s1 = name;
        int endOfSequence = 0;
        for (;;) {
          lpush(&ins->list, parse_operand(P));
          if (is_final(T(P), fin)) {
            endOfSequence = 1;
            break;
          } else if (tok_is(T(P), ",")) {
            P->idx++;
          } else if (tok_is(T(P), "\n")) {
            P->idx++;
            break;
          } else parse_error(P);
        }
        lpush(&seq->list, ins);
        if (endOfSequence) break;
      }
    } else if (is_identifier(t)) {
      const char *name = arena_strdup(tok_cstr(t, namebuf));
      P->idx++;
      if (!tok_is(T(P), "(")) parse_error(P);
      P->idx++;
      Node *call = mk(K_MACROCALL);
      call->s1 = name;
      skip_newline(P);
      if (tok_is(T(P), ")")) {
        P->idx++;
      } else {
        for (;;) {
          skip_newline(P);
          if (tok_is(T(P), "macro")) {
            P->idx++;
            Node *mac = mk(K_MACRO);
            mac->s1 = "";
            parse_macro_variables(P, mac);
            mac->a = parse_sequence(P, FIN_END);
            P->idx++;
            lpush(&call->list, mac);
          } else {
            lpush(&call->list, parse_operand(P));
          }
          skip_newline(P);
          if (tok_is(T(P), ")")) {
            P->idx++;
            break;
          } else if (tok_is(T(P), ",")) {
            P->idx++;
          } else parse_error(P);
        }
      }
      lpush(&seq->list, call);
    } else if (is_label(t) || is_local_label(t)) {
      int global_label = is_label(t);
      const char *name = arena_strdup(tok_cstr(t, namebuf));
      P->idx++;
      if (!tok_is(T(P), ":")) parse_error(P);
      lpush(&seq->list, intern(global_label ? K_LABEL : K_LOCALLABEL, name, NULL));
      P->idx++;
    } else if (tok_is(t, "include")) {
      P->idx++;
      if (!is_identifier(T(P))) parse_error(P);
      char modname[512];
      snprintf(modname, sizeof modname, "%s.asm", tok_cstr(T(P), namebuf));
      P->idx++;
      lpush(&seq->list, parse_file(modname));
    } else {
      parse_error(P);
    }
  }
  return seq;
}

static Node *parse_file(const char *name) {
  SourceFile *f = open_source(name);
  const char *saved = cur_file;
  cur_file = f->name;
  Parser P;
  P.tok = lex(f->data, f->len, &P.n);
  P.idx = 0;
  Node *seq = parse_sequence(&P, FIN_NONE);
  cur_file = saved;
  return seq;
}

/* ---------- dump (the JS dump() methods) ---------- */
typedef struct { char *p; size_t n, cap; } Buf;
static void bput_num(Buf *b, double v);

static void bput(Buf *b, const char *s) {
  size_t l = strlen(s);
  if (b->n + l + 1 > b->cap) {
    b->cap = (b->n + l + 1) * 2;
    b->p = realloc(b->p, b->cap);
    if (!b->p) die("out of memory");
  }
  memcpy(b->p + b->n, s, l);
  b->n += l;
  b->p[b->n] = '\0';
}

static void bput_ll(Buf *b, long long v) {
  char tmp[32];
  snprintf(tmp, sizeof tmp, "%lld", v);
  bput(b, tmp);
}

/* Number::toString for the integer-valued doubles the AST holds (shortest round-trip digits). */
static void num_text(double v, char *out) {
  char tmp[64];
  size_t o = 0;
  if (v != v || v > 1e21 || v < -1e21 || v != floor(v)) die("unsupported immediate value");
  if (v == 0) { strcpy(out, "0"); return; }
  if (v < 0) { out[o++] = '-'; v = -v; }
  int prec;
  for (prec = 1; prec <= 17; prec++) {
    snprintf(tmp, sizeof tmp, "%.*e", prec - 1, v);
    if (strtod(tmp, NULL) == v) break;
  }
  /* tmp is d[.ddd]e+XX: digits then exponent n-1 */
  char digits[32];
  int k = 0;
  char *e = strchr(tmp, 'e');
  for (char *q = tmp; q < e; q++) if (*q >= '0' && *q <= '9') digits[k++] = *q;
  while (k > 1 && digits[k - 1] == '0') k--;
  int n = atoi(e + 1) + 1;
  for (int i = 0; i < k; i++) out[o++] = digits[i];
  for (int i = k; i < n; i++) out[o++] = '0';
  out[o] = '\0';
}

static void bput_num(Buf *b, double v) {
  char tmp[64];
  num_text(v, tmp);
  bput(b, tmp);
}

static void dump(Node *n, Buf *b);

static void dump_list(const List *l, const char *sep, Buf *b) {
  int first = 1;
  for (Cell *c = l->head; c; c = c->next) {
    if (!first) bput(b, sep);
    first = 0;
    dump(c->node, b);
  }
}

static void dump(Node *n, Buf *b) {
  switch (n->kind) {
  case K_IMM: bput_num(b, n->ival); break;
  case K_ADD: bput(b, "("); dump(n->a, b); bput(b, " + "); dump(n->b, b); bput(b, ")"); break;
  case K_SUB: bput(b, "("); dump(n->a, b); bput(b, " - "); dump(n->b, b); bput(b, ")"); break;
  case K_MUL: bput(b, "("); dump(n->a, b); bput(b, " * "); dump(n->b, b); bput(b, ")"); break;
  case K_NEG: bput(b, "(-"); dump(n->a, b); bput(b, ")"); break;
  case K_OR: bput(b, "("); dump(n->a, b); bput(b, " | "); dump(n->b, b); bput(b, ")"); break;
  case K_AND: bput(b, "("); dump(n->a, b); bput(b, " & "); dump(n->b, b); bput(b, ")"); break;
  case K_XOR: bput(b, "("); dump(n->a, b); bput(b, " ^ "); dump(n->b, b); bput(b, ")"); break;
  case K_BITNOT: bput(b, "(~"); dump(n->a, b); bput(b, ")"); break;
  case K_STRLIT: bput(b, "\""); bput(b, n->s1); bput(b, "\""); break;
  case K_VAR: case K_SETTING: bput(b, n->s1); break;
  case K_LABEL: case K_LOCALLABEL: bput(b, n->s1); bput(b, ":"); break;
  case K_LABELREF: case K_LOCALLABELREF: bput(b, n->a->s1); break;
  case K_CONSTEXPR: bput(b, "constexpr ("); bput(b, n->s1); bput(b, ")"); break;
  case K_CONSTDECL: bput(b, "const "); dump(n->a, b); bput(b, " = "); dump(n->b, b); break;
  case K_STRUCTOFF: bput(b, n->s1); bput(b, "::"); bput(b, n->s2); break;
  case K_SIZEOF: bput(b, "sizeof "); bput(b, n->s1); break;
  case K_SEQ: dump_list(&n->list, "\n", b); break;
  case K_TRUE: bput(b, "true"); break;
  case K_FALSE: bput(b, "false"); break;
  case K_PAND: bput(b, "("); dump(n->a, b); bput(b, " and "); dump(n->b, b); bput(b, ")"); break;
  case K_POR: bput(b, "("); dump(n->a, b); bput(b, " or "); dump(n->b, b); bput(b, ")"); break;
  case K_PNOT: bput(b, "(not"); dump(n->a, b); bput(b, ")"); break;
  case K_SKIP: bput(b, "    skip"); break;
  case K_ERROR: bput(b, "    error"); break;
  case K_IFTE:
    bput(b, "if "); dump(n->a, b); bput(b, "\n");
    dump(n->b, b); bput(b, "\nelse\n"); dump(n->c, b); bput(b, "\nend");
    break;
  case K_MACRO:
    bput(b, "macro "); bput(b, n->s1); bput(b, "(");
    dump_list(&n->list2, ", ", b); bput(b, ")\n");
    dump(n->a, b); bput(b, "\nend");
    break;
  case K_MACROCALL:
    bput(b, "    "); bput(b, n->s1); bput(b, "(");
    dump_list(&n->list, ", ", b); bput(b, ")");
    break;
  case K_INSTR:
    bput(b, "    "); bput(b, n->s1); bput(b, " ");
    dump_list(&n->list, ", ", b);
    break;
  case K_ADDRESS: dump(n->b, b); bput(b, "["); dump(n->a, b); bput(b, "]"); break;
  case K_BASEINDEX:
    dump(n->c, b); bput(b, "["); dump(n->a, b); bput(b, ", "); dump(n->b, b);
    bput(b, ", "); bput_ll(b, n->scale); bput(b, "]");
    break;
  case K_ABSADDR: dump(n->a, b); bput(b, "[]"); break;
  default:
    die("dump: unknown node kind");
  }
}

/* ---------- expected lines (expected.js: expectedASTDumpedAsLines) ---------- */
static char **expected_lines;
static size_t expected_count;

static void load_expected(const char *root) {
  char path[4096];
  snprintf(path, sizeof path, "%s/RexBench/OfflineAssembler/expected.js", root);
  FILE *f = fopen(path, "rb");
  if (!f) { fprintf(stderr, "cannot open %s\n", path); exit(1); }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *src = malloc((size_t)n + 1);
  if (fread(src, 1, (size_t)n, f) != (size_t)n) die("short read");
  src[n] = '\0';
  fclose(f);
  const char *p = strstr(src, "expectedASTDumpedAsLines = [");
  if (!p) die("expected.js: no expectedASTDumpedAsLines");
  p += strlen("expectedASTDumpedAsLines = [");
  size_t cap = 1024;
  expected_lines = malloc(cap * sizeof *expected_lines);
  for (;;) {
    while (*p && *p != '"' && *p != ']') p++;
    if (*p != '"') break;
    p++;
    Buf b = {0};
    while (*p && !(*p == '"')) {
      if (*p == '\\' && p[1] == '"') { bput(&b, "\""); p += 2; continue; }
      char one[2] = {*p, 0};
      bput(&b, one);
      p++;
    }
    if (*p != '"') die("expected.js: unterminated line");
    p++;
    if (expected_count == cap) {
      cap *= 2;
      expected_lines = realloc(expected_lines, cap * sizeof *expected_lines);
    }
    expected_lines[expected_count++] = b.p ? b.p : strdup("");
  }
  free(src);
}

/* validate(): compares the dumped AST line by line with the expected lines. */
static void validate(Node *ast) {
  Buf b = {0};
  dump(ast, &b);
  size_t actual = 1;
  for (size_t i = 0; i < b.n; i++) if (b.p[i] == '\n') actual++;
  if (actual != expected_count) {
    fprintf(stderr, "validation failed: actual number of lines (%zu) differs from expected (%zu)\n",
            actual, expected_count);
    exit(1);
  }
  size_t index = 0, start = 0;
  for (size_t i = 0; i <= b.n; i++) {
    if (i == b.n || b.p[i] == '\n') {
      size_t len = i - start;
      const char *exp = expected_lines[index];
      if (strlen(exp) != len || memcmp(exp, b.p + start, len) != 0) {
        fprintf(stderr, "validation failed: line #%zu differs: expected \"%s\" got \"%.*s\"\n", index + 1, exp, (int)len, b.p + start);
        exit(1);
      }
      index++;
      start = i + 1;
    }
  }
  free(b.p);
}

int main(int argc, char **argv) {
  const char *root = argc > 2 ? argv[2] : default_root;
  int iterations = argc > 1 ? atoi(argv[1]) : DEFAULT_ITERATIONS;
  arena = malloc(ARENA_BYTES);
  if (!arena) die("cannot allocate arena");
  load_sources(root);
  load_expected(root);

  Node *ast = NULL;
  struct timespec a, b;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) {
    reset_ast();
    ast = parse_file("LowLevelInterpreter.asm");
  }
  clock_gettime(CLOCK_MONOTONIC, &b);
  double ms = (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6;

  validate(ast);
  printf("OfflineAssembler %d iterations %.3f ms (validation ok, %zu lines)\n", iterations, ms,
         expected_count);
  return 0;
}
