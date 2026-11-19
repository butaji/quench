// jetstream-row: Basic
// jetstream-files: ARES-6/Basic/ast.js ARES-6/Basic/basic.js ARES-6/Basic/caseless_map.js ARES-6/Basic/lexer.js ARES-6/Basic/number.js ARES-6/Basic/parser.js ARES-6/Basic/random.js ARES-6/Basic/state.js ARES-6/Basic/benchmark.js
// iterations: 120
/* C analogue of JetStream's ARES-6 Basic benchmark: the same interpreter design (a tokenizer that
 * produces tokens on demand, a recursive-descent parser that builds an AST with pushback, and a
 * tree-walking evaluator for numbers, strings, arrays and the statement set), run on the five BASIC
 * programs of ARES-6/Basic/benchmark.js. Each runIteration lexes, parses and runs the five programs
 * and compares their output with the expected text, as the JS does. Each program gets a fresh state
 * with the same fixed-seed RNG (Octane's Jenkins-hash generator), so the outputs are the JS ones.
 * Where the JS source has a bug on a path the benchmark never reaches, this port does what the code
 * plainly intends: GOTO/GOSUB set kind instead of process, ON indexes targets[index-1], FOR's skip
 * assigns a property of the statement instead of the state, RETURN/ON/FOR use this instead of state,
 * INPUT/READ have a `stament` typo, DEF passes the token as the name and calls this.code. The
 * programs here never reach these paths, so the outputs are unaffected.
 * Deviations: the AST is allocated from an arena and freed per program, JS objects go to the GC.
 * Usage: Basic [iterations] */
#include <ctype.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define FAIL(...)                       \
  do {                                  \
    fprintf(stderr, __VA_ARGS__);       \
    fputc('\n', stderr);                \
    exit(1);                            \
  } while (0)

/* ---------- growable text buffer ---------- */

typedef struct { char *data; size_t len, cap; } Buf;

static void buf_reserve(Buf *b, size_t extra) {
  if (b->len + extra + 1 > b->cap) {
    size_t cap = b->cap ? b->cap : 256;
    while (cap < b->len + extra + 1) cap *= 2;
    char *p = realloc(b->data, cap);
    if (!p) FAIL("out of memory");
    b->data = p;
    b->cap = cap;
  }
}

static void buf_append(Buf *b, const char *s, size_t n) {
  buf_reserve(b, n);
  if (n) memcpy(b->data + b->len, s, n);
  b->len += n;
  b->data[b->len] = '\0';
}

static void buf_putc(Buf *b, char c) { buf_append(b, &c, 1); }

static void buf_free(Buf *b) {
  free(b->data);
  b->data = NULL;
  b->len = b->cap = 0;
}

/* ---------- arena: the AST and interned names live until the program is done ---------- */

typedef struct Chunk { struct Chunk *next; char *mem; size_t used, cap; } Chunk;
static Chunk *arena_head;

static void *arena_alloc(size_t n) {
  n = (n + 15) & ~(size_t)15;
  if (!arena_head || arena_head->used + n > arena_head->cap) {
    size_t cap = n > 65536 ? n : 65536;
    Chunk *c = malloc(sizeof *c);
    if (!c) FAIL("out of memory");
    c->mem = malloc(cap);
    if (!c->mem) FAIL("out of memory");
    c->used = 0;
    c->cap = cap;
    c->next = arena_head;
    arena_head = c;
  }
  void *p = arena_head->mem + arena_head->used;
  arena_head->used += n;
  return p;
}

static void *arena_zalloc(size_t n) {
  void *p = arena_alloc(n);
  memset(p, 0, n);
  return p;
}

static char *arena_strndup(const char *s, size_t n) {
  char *p = arena_alloc(n + 1);
  memcpy(p, s, n);
  p[n] = '\0';
  return p;
}

static const char *arena_lower(const char *s) {
  size_t n = strlen(s);
  char *p = arena_strndup(s, n);
  for (size_t i = 0; i < n; i++) p[i] = (char)tolower((unsigned char)p[i]);
  return p;
}

/* Grows an arena array of n elements of size elem to hold one more. */
static void *arena_grow(void *arr, int *cap, int n, size_t elem) {
  if (n < *cap) return arr;
  int nc = *cap ? *cap * 2 : 4;
  void *p = arena_alloc((size_t)nc * elem);
  if (n) memcpy(p, arr, (size_t)n * elem);
  *cap = nc;
  return p;
}

static void arena_release(void) {
  Chunk *c = arena_head;
  while (c) {
    Chunk *next = c->next;
    free(c->mem);
    free(c);
    c = next;
  }
  arena_head = NULL;
}

/* ---------- ECMAScript Number::toString (shortest round-trip digits) ---------- */

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

/* JS truthiness for numbers: 0 and NaN are false. */
static int truthy(double x) { return x != 0 && x == x; }

/* Math.max(a, b) and Math.round(x) as JS defines them for the values used here. */
static double js_max(double a, double b) {
  if (a != a || b != b) return NAN;
  return a >= b ? a : b;
}
static double js_round(double x) { return floor(x + 0.5); }
static double js_sign(double x) { return x > 0 ? 1 : (x < 0 ? -1 : x); }

/* ---------- tokens and lexer (lexer.js) ---------- */

typedef enum { TK_USERLINE, TK_KEYWORD, TK_IDENT, TK_NUMBER, TK_STRING, TK_OPERATOR, TK_NEWLINE, TK_EOF } TokKind;
#define TOK_TEXT_MAX 256

typedef struct {
  TokKind kind;
  char text[TOK_TEXT_MAX];
  double value;
  int source_line;
} Token;

typedef struct {
  const char *src;
  size_t len;
  size_t next_line_start;
  int source_line;
  int in_line;
  const char *line;
  size_t line_len, pos;
} Lexer;

/* Keywords in the order of the JS alternation: the first prefix match wins, so "gosub" lexes as
 * "go" + "sub", and identifiers that start with a keyword are split (as the JS regexes do). */
static const char *const KEYWORDS[] = {"base", "data", "def", "dim", "end", "for", "go", "gosub", "goto",
                                       "if", "input", "let", "next", "on", "option", "print", "randomize",
                                       "read", "restore", "return", "step", "stop", "sub", "then", "to", NULL};

static size_t match_keyword(const char *s, size_t n, const char *kw) {
  size_t k = strlen(kw);
  if (n < k) return 0;
  for (size_t i = 0; i < k; i++)
    if (tolower((unsigned char)s[i]) != kw[i]) return 0;
  return k;
}

static int is_ws(char c) { return c == ' ' || c == '\t' || c == '\r' || c == '\v' || c == '\f'; }

/* Longest string literal per /^"([^"]|"")*"/: a doubled quote is content, a lone quote closes. */
static size_t match_string(const char *r, size_t rem) {
  if (rem == 0 || r[0] != '"') return 0;
  size_t i = 1, end = 0;
  while (i < rem) {
    if (r[i] == '"') {
      end = i + 1;
      if (i + 1 < rem && r[i + 1] == '"') {
        i += 2;
        continue;
      }
      break;
    }
    i++;
  }
  return end;
}

static void lexer_init(Lexer *L, const char *src, size_t len) {
  memset(L, 0, sizeof *L);
  L->src = src;
  L->len = len;
}

static void tok_set(Token *t, TokKind kind, const char *s, size_t n, int source_line) {
  if (n >= TOK_TEXT_MAX) FAIL("At %d: token too long", source_line);
  t->kind = kind;
  memcpy(t->text, s, n);
  t->text[n] = '\0';
  t->value = 0;
  t->source_line = source_line;
}

/* Produces the next token; returns 0 when the source is exhausted. Lines are read lazily, so a
 * malformed trailing line is never reached when the parser stops at END. */
static int lex_next(Lexer *L, Token *t) {
  if (!L->in_line) {
    if (L->next_line_start > L->len) return 0;
    size_t start = L->next_line_start, end = start;
    while (end < L->len && L->src[end] != '\n') end++;
    L->line = L->src + start;
    L->line_len = end - start;
    L->pos = 0;
    L->next_line_start = end < L->len ? end + 1 : L->len + 1;
    L->source_line++;
    L->in_line = 1;
    size_t p = 0;
    while (p < L->line_len && is_ws(L->line[p])) p++;
    if (!(p < L->line_len && isdigit((unsigned char)L->line[p])))
      FAIL("At line %d: Expect line number: %.*s", L->source_line, (int)L->line_len, L->line);
    size_t d = p;
    while (d < L->line_len && isdigit((unsigned char)L->line[d])) d++;
    tok_set(t, TK_USERLINE, L->line + p, d - p, L->source_line);
    t->value = strtod(t->text, NULL);
    L->pos = d;
    return 1;
  }
  while (L->pos < L->line_len && is_ws(L->line[L->pos])) L->pos++;
  if (L->pos >= L->line_len) {
    tok_set(t, TK_NEWLINE, "\n", 1, L->source_line);
    L->in_line = 0;
    return 1;
  }
  const char *r = L->line + L->pos;
  size_t rem = L->line_len - L->pos, n = 0;
  TokKind kind = TK_OPERATOR;
  int found = 0;
  for (int k = 0; KEYWORDS[k] && !found; k++) {
    n = match_keyword(r, rem, KEYWORDS[k]);
    if (n) {
      kind = TK_KEYWORD;
      found = 1;
    }
  }
  unsigned char c0 = (unsigned char)r[0];
  if (!found && (isalpha(c0) || c0 == '_')) {
    kind = TK_IDENT;
    n = 1;
    while (n < rem && (isalnum((unsigned char)r[n]) || r[n] == '_')) n++;
    found = 1;
  } else if (!found && isdigit(c0)) {
    kind = TK_NUMBER;
    n = 0;
    while (n < rem && isdigit((unsigned char)r[n])) n++;
    if (n < rem && r[n] == '.') {
      n++;
      while (n < rem && isdigit((unsigned char)r[n])) n++;
    }
    found = 1;
  } else if (!found && c0 == '.' && rem > 1 && isdigit((unsigned char)r[1])) {
    kind = TK_NUMBER;
    n = 1;
    while (n < rem && isdigit((unsigned char)r[n])) n++;
    if (n < rem && (r[n] == 'e' || r[n] == 'E')) {
      size_t m = n + 1;
      if (m < rem && (r[m] == '+' || r[m] == '-')) m++;
      if (m < rem && isdigit((unsigned char)r[m])) {
        while (m < rem && isdigit((unsigned char)r[m])) m++;
        n = m;
      }
    }
    found = 1;
  } else if (!found && (n = match_string(r, rem)) != 0) {
    kind = TK_STRING;
    found = 1;
  }
  if (!found) {
    if (strchr("-+*/^()=,$;", c0) && c0 != '\0') {
      n = 1;
    } else if (c0 == '<') {
      n = (rem > 1 && (r[1] == '>' || r[1] == '=')) ? 2 : 1;
    } else if (c0 == '>') {
      n = (rem > 1 && r[1] == '=') ? 2 : 1;
    } else {
      FAIL("At line %d: Cannot lex token: %.*s", L->source_line, (int)rem, r);
    }
  }
  tok_set(t, kind, r, n, L->source_line);
  if (kind == TK_NUMBER) t->value = strtod(t->text, NULL);
  L->pos += n;
  return 1;
}

/* ---------- AST (ast.js) ---------- */

enum {
  EX_NUM_CONST, EX_NUM_APPLY, EX_VARIABLE, EX_POW, EX_MUL, EX_DIV, EX_NEG, EX_ADD, EX_SUB,
  EX_EQ, EX_NE, EX_LT, EX_GT, EX_LE, EX_GE, EX_STR_CONST, EX_STR_VAR, EX_STR_EQ, EX_STR_NE
};
#define MAX_ARGS 8

typedef struct Expr {
  int kind;
  double value;                /* EX_NUM_CONST */
  const char *str;             /* EX_STR_CONST */
  size_t slen;
  const char *name, *lname;    /* EX_NUM_APPLY, EX_VARIABLE, EX_STR_VAR; lname is the lookup key */
  struct Expr *args[MAX_ARGS]; /* call or subscript arguments */
  int nargs;
  struct Expr *left, *right;   /* binary operators; EX_NEG uses left */
} Expr;

enum {
  SK_NONE, SK_DEF, SK_LET, SK_GOTO, SK_GOSUB, SK_IF, SK_RETURN, SK_STOP, SK_ON, SK_FOR, SK_NEXT,
  SK_PRINT, SK_INPUT, SK_READ, SK_RESTORE, SK_DIM, SK_RANDOMIZE, SK_END
};
enum { PI_COMMA, PI_TAB, PI_STRING, PI_NUMBER };

typedef struct { int kind; Expr *value; } PrintItem;
typedef struct { const char *name, *lname; int nbounds; int bounds[2]; } DimItem;

typedef struct Stmt {
  int kind;
  long line_number;
  int source_line;
  const char *name, *lname;      /* DEF, FOR and NEXT variable */
  const char *params[MAX_ARGS];  /* DEF parameters (lowercase) */
  int nparams;
  Expr *expr;                    /* DEF body, LET value, ON selector */
  Expr *variable;                /* LET target */
  Expr *condition;               /* IF */
  long target;                   /* GOTO, GOSUB, IF */
  long *targets;                 /* ON */
  int ntargets, captargets;
  Expr *initial, *limit, *step;  /* FOR */
  struct Stmt *loop_next;        /* FOR: its NEXT */
  struct Stmt *loop_for;         /* NEXT: its FOR */
  double *for_var;               /* FOR runtime state (JS sideState) */
  double for_limit, for_step;
  PrintItem *items;              /* PRINT */
  int nitems, capitems;
  Expr **vars;                   /* INPUT, READ */
  int nvars, capvars;
  DimItem *dims;                 /* DIM */
  int ndims, capdims;
} Stmt;

typedef struct {
  Stmt **lines;   /* statement at each line number, NULL when none */
  long nlines;    /* allocated length of lines */
  long max_line;
  int base;       /* OPTION BASE */
  Expr **data;
  int ndata, capdata;
} Program;

static void set_statement(Program *p, Stmt *s) {
  long n = s->line_number;
  if (n + 1 > p->nlines) {
    long nn = n + 1;
    Stmt **a = realloc(p->lines, (size_t)nn * sizeof *a);
    if (!a) FAIL("out of memory");
    for (long i = p->nlines; i < nn; i++) a[i] = NULL;
    p->lines = a;
    p->nlines = nn;
  }
  p->lines[n] = s;
  if (n > p->max_line) p->max_line = n;
}

static Stmt *lookup_statement(const Program *p, long n) {
  return (n >= 0 && n < p->nlines) ? p->lines[n] : NULL;
}

static Expr *new_expr(int kind) {
  Expr *e = arena_zalloc(sizeof *e);
  e->kind = kind;
  return e;
}

static Expr *new_binary(int kind, Expr *l, Expr *r) {
  Expr *e = new_expr(kind);
  e->left = l;
  e->right = r;
  return e;
}

static Expr *new_named(int kind, const char *name) {
  Expr *e = new_expr(kind);
  e->name = arena_strndup(name, strlen(name));
  e->lname = arena_lower(name);
  return e;
}

static Expr *new_num_const(double v) {
  Expr *e = new_expr(EX_NUM_CONST);
  e->value = v;
  return e;
}

/* Decodes a string literal token: the quotes are removed and a doubled quote becomes one quote. */
static Expr *new_string_const(const Token *t) {
  Expr *e = new_expr(EX_STR_CONST);
  size_t n = strlen(t->text);
  char *buf = arena_alloc(n + 1);
  size_t k = 0;
  for (size_t i = 1; i + 1 < n; i++) {
    char c = t->text[i];
    if (c == '"') i++;
    buf[k++] = c;
  }
  buf[k] = '\0';
  e->str = buf;
  e->slen = k;
  return e;
}

/* ---------- parser (parser.js) ---------- */

typedef struct {
  Lexer lex;
  Token pushback[32];
  int npush;
  Program *prog;
} Parser;

static void next_tok(Parser *P, Token *t) {
  if (P->npush > 0) {
    *t = P->pushback[--P->npush];
    return;
  }
  if (!lex_next(&P->lex, t)) {
    memset(t, 0, sizeof *t);
    t->kind = TK_EOF;
    strcpy(t->text, "<end of file>");
    t->source_line = P->lex.source_line;
  }
}

static void push_tok(Parser *P, const Token *t) {
  if (P->npush == 32) FAIL("parser pushback overflow");
  P->pushback[P->npush++] = *t;
}

static void peek_tok(Parser *P, Token *t) {
  next_tok(P, t);
  push_tok(P, t);
}

static int peek_is(Parser *P, const char *s) {
  Token t;
  peek_tok(P, &t);
  return strcmp(t.text, s) == 0;
}

static int ci_equal(const char *a, const char *b) {
  for (;; a++, b++) {
    if (tolower((unsigned char)*a) != tolower((unsigned char)*b)) return 0;
    if (!*a) return 1;
  }
}

static void consume_kind(Parser *P, TokKind k, Token *t) {
  next_tok(P, t);
  if (t->kind != k) FAIL("At %d: expected token kind %d but got: %s", t->source_line, (int)k, t->text);
}

static void consume_word(Parser *P, const char *s) {
  Token t;
  next_tok(P, &t);
  if (!ci_equal(t.text, s)) FAIL("At %d: expected %s but got: %s", t.source_line, s, t.text);
}

static Expr *parse_numeric(Parser *P);

/* Parses "(" args ")" after a name whose next token is "(". */
static void parse_args(Parser *P, Expr *e) {
  do {
    Token t;
    next_tok(P, &t); /* "(" or "," */
    if (e->nargs == MAX_ARGS) FAIL("too many arguments");
    e->args[e->nargs++] = parse_numeric(P);
  } while (peek_is(P, ","));
  consume_word(P, ")");
}

static Expr *parse_variable(Parser *P) {
  Token t;
  consume_kind(P, TK_IDENT, &t);
  Expr *e = new_named(EX_VARIABLE, t.text);
  if (peek_is(P, "(")) parse_args(P, e);
  return e;
}

static Expr *parse_primary(Parser *P) {
  Token t;
  next_tok(P, &t);
  if (t.kind == TK_IDENT) {
    Expr *e = new_named(EX_NUM_APPLY, t.text);
    if (peek_is(P, "(")) parse_args(P, e);
    return e;
  }
  if (t.kind == TK_NUMBER) return new_num_const(t.value);
  if (t.kind == TK_OPERATOR && strcmp(t.text, "(") == 0) {
    Expr *e = parse_numeric(P);
    consume_word(P, ")");
    return e;
  }
  FAIL("At %d: expected identifier, number, or (, but got: %s", t.source_line, t.text);
  return NULL;
}

static Expr *parse_factor(Parser *P) {
  Expr *primary = parse_primary(P);
  while (peek_is(P, "^")) {
    Token t;
    next_tok(P, &t);
    primary = new_binary(EX_POW, primary, parse_primary(P));
  }
  return primary;
}

static Expr *parse_term(Parser *P) {
  Expr *factor = parse_factor(P);
  for (;;) {
    if (peek_is(P, "*")) {
      Token t;
      next_tok(P, &t);
      factor = new_binary(EX_MUL, factor, parse_factor(P));
    } else if (peek_is(P, "/")) {
      Token t;
      next_tok(P, &t);
      factor = new_binary(EX_DIV, factor, parse_factor(P));
    } else {
      return factor;
    }
  }
}

/* Only the leading term may have a sign. */
static Expr *parse_numeric(Parser *P) {
  int negate = 0;
  if (peek_is(P, "+")) {
    Token t;
    next_tok(P, &t);
  } else if (peek_is(P, "-")) {
    Token t;
    next_tok(P, &t);
    negate = 1;
  }
  Expr *term = parse_term(P);
  if (negate) {
    Expr *n = new_expr(EX_NEG);
    n->left = term;
    term = n;
  }
  for (;;) {
    if (peek_is(P, "+")) {
      Token t;
      next_tok(P, &t);
      term = new_binary(EX_ADD, term, parse_term(P));
    } else if (peek_is(P, "-")) {
      Token t;
      next_tok(P, &t);
      term = new_binary(EX_SUB, term, parse_term(P));
    } else {
      return term;
    }
  }
}

static int is_string_expression(Parser *P) {
  Token t;
  next_tok(P, &t);
  int result = 0;
  if (t.kind == TK_STRING) {
    result = 1;
  } else if (t.kind == TK_IDENT) {
    result = peek_is(P, "$");
  }
  push_tok(P, &t);
  return result;
}

static Expr *parse_string_expression(Parser *P) {
  Token t;
  next_tok(P, &t);
  if (t.kind == TK_STRING) return new_string_const(&t);
  if (t.kind == TK_IDENT) {
    consume_word(P, "$");
    return new_named(EX_STR_VAR, t.text);
  }
  FAIL("At %d: expected string expression but got %s", t.source_line, t.text);
  return NULL;
}

static Expr *parse_relational(Parser *P) {
  if (is_string_expression(P)) {
    Expr *left = parse_string_expression(P);
    Token op;
    next_tok(P, &op);
    int kind;
    if (strcmp(op.text, "=") == 0) kind = EX_STR_EQ;
    else if (strcmp(op.text, "<>") == 0) kind = EX_STR_NE;
    else FAIL("At %d: expected a string comparison operator but got: %s", op.source_line, op.text);
    return new_binary(kind, left, parse_string_expression(P));
  }
  Expr *left = parse_numeric(P);
  Token op;
  next_tok(P, &op);
  int kind;
  if (strcmp(op.text, "=") == 0) kind = EX_EQ;
  else if (strcmp(op.text, "<>") == 0) kind = EX_NE;
  else if (strcmp(op.text, "<") == 0) kind = EX_LT;
  else if (strcmp(op.text, ">") == 0) kind = EX_GT;
  else if (strcmp(op.text, "<=") == 0) kind = EX_LE;
  else if (strcmp(op.text, ">=") == 0) kind = EX_GE;
  else FAIL("At %d: expected a numeric comparison operator but got: %s", op.source_line, op.text);
  return new_binary(kind, left, parse_numeric(P));
}

static long parse_nonneg(Parser *P) {
  Token t;
  next_tok(P, &t);
  size_t n = strlen(t.text);
  if (n == 0) FAIL("At %d: expected a line number", t.source_line);
  for (size_t i = 0; i < n; i++)
    if (!isdigit((unsigned char)t.text[i]))
      FAIL("At %d: expected a line number but got: %s", t.source_line, t.text);
  return strtol(t.text, NULL, 10);
}

static Expr *parse_constant(Parser *P) {
  Token t;
  if (peek_is(P, "+")) {
    next_tok(P, &t);
    consume_kind(P, TK_NUMBER, &t);
    return new_num_const(t.value);
  }
  if (peek_is(P, "-")) {
    next_tok(P, &t);
    consume_kind(P, TK_NUMBER, &t);
    return new_num_const(-t.value);
  }
  if (is_string_expression(P)) {
    consume_kind(P, TK_STRING, &t);
    return new_string_const(&t);
  }
  consume_kind(P, TK_NUMBER, &t);
  return new_num_const(t.value);
}

static void add_print_item(Stmt *s, int kind, Expr *value) {
  s->items = arena_grow(s->items, &s->capitems, s->nitems, sizeof(PrintItem));
  s->items[s->nitems].kind = kind;
  s->items[s->nitems].value = value;
  s->nitems++;
}

static void add_var(Stmt *s, Expr *v) {
  s->vars = arena_grow(s->vars, &s->capvars, s->nvars, sizeof(Expr *));
  s->vars[s->nvars++] = v;
}

static void add_target(Stmt *s, long t) {
  s->targets = arena_grow(s->targets, &s->captargets, s->ntargets, sizeof(long));
  s->targets[s->ntargets++] = t;
}

static void add_dim(Stmt *s, DimItem d) {
  s->dims = arena_grow(s->dims, &s->capdims, s->ndims, sizeof(DimItem));
  s->dims[s->ndims++] = d;
}

static void add_data(Parser *P, Expr *v) {
  Program *p = P->prog;
  p->data = arena_grow(p->data, &p->capdata, p->ndata, sizeof(Expr *));
  p->data[p->ndata++] = v;
}

static Stmt *parse_statements(Parser *P);

static Stmt *parse_statement(Parser *P) {
  Token t;
  consume_kind(P, TK_USERLINE, &t);
  Stmt *s = arena_zalloc(sizeof *s);
  s->line_number = (long)t.value;
  set_statement(P->prog, s);
  Token cmd;
  next_tok(P, &cmd);
  s->source_line = cmd.source_line;
  if (cmd.kind != TK_KEYWORD)
    FAIL("At %d: expected command but got: %s (of kind %d)", cmd.source_line, cmd.text, (int)cmd.kind);
  char kw[32];
  size_t kl = strlen(cmd.text);
  if (kl >= sizeof kw) FAIL("At %d: unexpected command: %s", cmd.source_line, cmd.text);
  for (size_t i = 0; i <= kl; i++) kw[i] = (char)tolower((unsigned char)cmd.text[i]);
  int early_return = 0;

  if (!strcmp(kw, "def")) {
    s->kind = SK_DEF;
    Token name;
    consume_kind(P, TK_IDENT, &name);
    s->name = arena_strndup(name.text, strlen(name.text));
    s->lname = arena_lower(name.text);
    if (peek_is(P, "(")) {
      do {
        Token t2;
        next_tok(P, &t2);
        Token param;
        consume_kind(P, TK_IDENT, &param);
        if (s->nparams == MAX_ARGS) FAIL("At %d: too many parameters", cmd.source_line);
        s->params[s->nparams++] = arena_lower(param.text);
      } while (peek_is(P, ","));
    }
    s->expr = parse_numeric(P);
  } else if (!strcmp(kw, "let")) {
    s->kind = SK_LET;
    s->variable = parse_variable(P);
    consume_word(P, "=");
    s->expr = parse_numeric(P);
  } else if (!strcmp(kw, "go")) {
    Token next;
    next_tok(P, &next);
    if (strcmp(next.text, "to") == 0) {
      s->kind = SK_GOTO;
      s->target = parse_nonneg(P);
    } else if (strcmp(next.text, "sub") == 0) {
      s->kind = SK_GOSUB;
      s->target = parse_nonneg(P);
    } else {
      FAIL("At %d: expected to or sub but got: %s", next.source_line, next.text);
    }
  } else if (!strcmp(kw, "goto")) {
    s->kind = SK_GOTO;
    s->target = parse_nonneg(P);
  } else if (!strcmp(kw, "gosub")) {
    s->kind = SK_GOSUB;
    s->target = parse_nonneg(P);
  } else if (!strcmp(kw, "if")) {
    s->kind = SK_IF;
    s->condition = parse_relational(P);
    consume_word(P, "then");
    s->target = parse_nonneg(P);
  } else if (!strcmp(kw, "return")) {
    s->kind = SK_RETURN;
  } else if (!strcmp(kw, "stop")) {
    s->kind = SK_STOP;
  } else if (!strcmp(kw, "on")) {
    s->kind = SK_ON;
    s->expr = parse_numeric(P);
    if (peek_is(P, "go")) {
      consume_word(P, "go");
      consume_word(P, "to");
    } else {
      consume_word(P, "goto");
    }
    for (;;) {
      add_target(s, parse_nonneg(P));
      if (!peek_is(P, ",")) break;
      Token comma;
      next_tok(P, &comma);
    }
  } else if (!strcmp(kw, "for")) {
    s->kind = SK_FOR;
    Token v;
    consume_kind(P, TK_IDENT, &v);
    s->name = arena_strndup(v.text, strlen(v.text));
    s->lname = arena_lower(v.text);
    consume_word(P, "=");
    s->initial = parse_numeric(P);
    consume_word(P, "to");
    s->limit = parse_numeric(P);
    if (peek_is(P, "step")) {
      Token st;
      next_tok(P, &st);
      s->step = parse_numeric(P);
    } else {
      s->step = new_num_const(1);
    }
    consume_kind(P, TK_NEWLINE, &t);
    Stmt *last = parse_statements(P);
    if (last->kind != SK_NEXT) FAIL("At %d: expected next statement", last->source_line);
    if (strcmp(last->name, s->name) != 0)
      FAIL("At %d: expected next for %s but got %s", last->source_line, s->name, last->name);
    last->loop_for = s;
    s->loop_next = last;
    early_return = 1;
  } else if (!strcmp(kw, "next")) {
    s->kind = SK_NEXT;
    Token v;
    consume_kind(P, TK_IDENT, &v);
    s->name = arena_strndup(v.text, strlen(v.text));
    s->lname = arena_lower(v.text);
  } else if (!strcmp(kw, "print")) {
    s->kind = SK_PRINT;
    for (;;) {
      if (peek_is(P, ",")) {
        Token c;
        next_tok(P, &c);
        add_print_item(s, PI_COMMA, NULL);
      } else if (peek_is(P, ";")) {
        Token c;
        next_tok(P, &c);
      } else if (peek_is(P, "tab")) {
        Token c;
        next_tok(P, &c);
        consume_word(P, "(");
        add_print_item(s, PI_TAB, parse_numeric(P));
      } else if (peek_is(P, "\n")) {
        break;
      } else if (is_string_expression(P)) {
        add_print_item(s, PI_STRING, parse_string_expression(P));
      } else {
        add_print_item(s, PI_NUMBER, parse_numeric(P));
      }
    }
  } else if (!strcmp(kw, "input") || !strcmp(kw, "read")) {
    s->kind = kw[0] == 'i' ? SK_INPUT : SK_READ;
    for (;;) {
      add_var(s, parse_variable(P));
      if (!peek_is(P, ",")) break;
      Token c;
      next_tok(P, &c);
    }
  } else if (!strcmp(kw, "restore")) {
    s->kind = SK_RESTORE;
  } else if (!strcmp(kw, "data")) {
    for (;;) {
      add_data(P, parse_constant(P));
      if (!peek_is(P, ",")) break;
      Token c;
      next_tok(P, &c);
    }
  } else if (!strcmp(kw, "dim")) {
    s->kind = SK_DIM;
    for (;;) {
      Token v;
      consume_kind(P, TK_IDENT, &v);
      DimItem d;
      memset(&d, 0, sizeof d);
      d.name = arena_strndup(v.text, strlen(v.text));
      d.lname = arena_lower(v.text);
      consume_word(P, "(");
      d.bounds[0] = (int)parse_nonneg(P);
      d.nbounds = 1;
      if (peek_is(P, ",")) {
        Token c;
        next_tok(P, &c);
        d.bounds[1] = (int)parse_nonneg(P);
        d.nbounds = 2;
      }
      consume_word(P, ")");
      add_dim(s, d);
      if (!peek_is(P, ",")) break;
      consume_word(P, ",");
    }
  } else if (!strcmp(kw, "option")) {
    consume_word(P, "base");
    long base = parse_nonneg(P);
    if (base != 0 && base != 1) FAIL("At %d: unexpected base: %ld", cmd.source_line, base);
    P->prog->base = (int)base;
  } else if (!strcmp(kw, "randomize")) {
    s->kind = SK_RANDOMIZE;
  } else if (!strcmp(kw, "end")) {
    s->kind = SK_END;
  } else {
    FAIL("At %d: unexpected command but got: %s", cmd.source_line, cmd.text);
  }

  if (early_return) return s;
  consume_kind(P, TK_NEWLINE, &t);
  return s;
}

static Stmt *parse_statements(Parser *P) {
  Stmt *s;
  do {
    s = parse_statement(P);
  } while (!(s->kind == SK_NEXT || s->kind == SK_END));
  return s;
}

static Program *parse_program(const char *src) {
  Parser *P = calloc(1, sizeof *P);
  Program *prog = calloc(1, sizeof *prog);
  if (!P || !prog) FAIL("out of memory");
  P->prog = prog;
  lexer_init(&P->lex, src, strlen(src));
  Stmt *last = parse_statements(P);
  if (last->kind != SK_END) FAIL("At %d: expected end", last->source_line);
  free(P);
  return prog;
}

static void free_program(Program *p) {
  free(p->lines);
  free(p->data);
  free(p);
}

/* ---------- runtime (state.js, number.js, random.js) ---------- */


enum { V_SCALAR, V_ARRAY, V_NATIVE, V_FUNC };
enum { NAT_ABS, NAT_ATN, NAT_COS, NAT_EXP, NAT_INT, NAT_LOG, NAT_RND, NAT_SGN, NAT_SIN, NAT_SQR, NAT_TAN };

typedef struct Var {
  int kind;
  double value;                /* V_SCALAR */
  int ndims;                   /* V_ARRAY */
  size_t dims[MAX_ARGS];
  double *cells;
  size_t ncells;
  int native, arity;           /* V_NATIVE */
  const char **params;         /* V_FUNC */
  int nparams;
  const Expr *body;
} Var;

typedef struct { const char *lname; Var *var; } Slot;

typedef struct Frame {
  const char **names;
  double *values;
  int n;
  struct Frame *up;
} Frame;

typedef struct {
  Program *prog;
  Slot *slots;
  size_t nslots, capslots;
  Frame *frame;
  uint32_t rng;
  long next_line;
  int has_next;
  long *substack;
  size_t nsub, capsub;
  long data_index;
  Buf out;
} State;

/* createRNG: Robert Jenkins' 32-bit hash over a seed, with the 28-bit output of random.js. */
static double rng_next(State *st) {
  uint32_t seed = st->rng;
  seed = (seed + 0x7ed55d16u) + (seed << 12);
  seed = (seed ^ 0xc761c23cu) ^ (seed >> 19);
  seed = (seed + 0x165667b1u) + (seed << 5);
  seed = (seed + 0xd3a2646cu) ^ (seed << 9);
  seed = (seed + 0xfd7046c5u) + (seed << 3);
  seed = (seed ^ 0xb55a4f09u) ^ (seed >> 16);
  st->rng = seed;
  return (double)(seed & 0xfffffffu) / 268435456.0;
}

static void add_native(State *st, const char *lname, int id, int arity) {
  Var *v = calloc(1, sizeof *v);
  if (!v) FAIL("out of memory");
  v->kind = V_NATIVE;
  v->native = id;
  v->arity = arity;
  if (st->nslots == st->capslots) {
    size_t nc = st->capslots ? st->capslots * 2 : 32;
    Slot *a = realloc(st->slots, nc * sizeof *a);
    if (!a) FAIL("out of memory");
    st->slots = a;
    st->capslots = nc;
  }
  st->slots[st->nslots].lname = lname;
  st->slots[st->nslots].var = v;
  st->nslots++;
}

static void state_init(State *st, Program *p) {
  memset(st, 0, sizeof *st);
  st->prog = p;
  st->rng = 49734321u; /* Octane's initial seed */
  st->has_next = 1;
  add_native(st, "abs", NAT_ABS, 1);
  add_native(st, "atn", NAT_ATN, 1);
  add_native(st, "cos", NAT_COS, 1);
  add_native(st, "exp", NAT_EXP, 1);
  add_native(st, "int", NAT_INT, 1);
  add_native(st, "log", NAT_LOG, 1);
  add_native(st, "rnd", NAT_RND, 0);
  add_native(st, "sgn", NAT_SGN, 1);
  add_native(st, "sin", NAT_SIN, 1);
  add_native(st, "sqr", NAT_SQR, 1);
  add_native(st, "tan", NAT_TAN, 1);
}

static void state_release(State *st) {
  for (size_t i = 0; i < st->nslots; i++) {
    Var *v = st->slots[i].var;
    if (v->kind == V_ARRAY) free(v->cells);
    free(v);
  }
  free(st->slots);
  free(st->substack);
  buf_free(&st->out);
}

static Var *find_global(State *st, const char *lname) {
  for (size_t i = 0; i < st->nslots; i++)
    if (strcmp(st->slots[i].lname, lname) == 0) return st->slots[i].var;
  return NULL;
}

static void add_global(State *st, const char *lname, Var *v) {
  if (st->nslots == st->capslots) {
    size_t nc = st->capslots ? st->capslots * 2 : 32;
    Slot *a = realloc(st->slots, nc * sizeof *a);
    if (!a) FAIL("out of memory");
    st->slots = a;
    st->capslots = nc;
  }
  st->slots[st->nslots].lname = lname;
  st->slots[st->nslots].var = v;
  st->nslots++;
}

/* State.getValue: an unknown name becomes a scalar, or an array with 11 cells per subscript. */
static Var *get_value(State *st, const char *lname, int nparams) {
  Var *v = find_global(st, lname);
  if (v) return v;
  v = calloc(1, sizeof *v);
  if (!v) FAIL("out of memory");
  if (nparams == 0) {
    v->kind = V_SCALAR;
  } else {
    v->kind = V_ARRAY;
    v->ndims = nparams;
    size_t total = 1;
    for (int i = 0; i < nparams; i++) {
      v->dims[i] = 11;
      total *= 11;
    }
    v->cells = calloc(total, sizeof(double));
    if (!v->cells) FAIL("out of memory");
    v->ncells = total;
  }
  add_global(st, lname, v);
  return v;
}

static int frame_lookup(State *st, const char *lname, double **cell) {
  for (Frame *f = st->frame; f; f = f->up)
    for (int i = 0; i < f->n; i++)
      if (strcmp(f->names[i], lname) == 0) {
        *cell = &f->values[i];
        return 1;
      }
  return 0;
}

/* NumberArray.leftApply: each subscript is floored and checked against its dimension. */
static double *array_cell(State *st, Var *v, const double *idx, int n) {
  if (v->ndims != n) FAIL("Expected %d arguments but %d were passed.", v->ndims, n);
  size_t off = 0;
  for (int k = 0; k < n; k++) {
    double ix = floor(idx[k]);
    if (!(ix >= st->prog->base) || !(ix < (double)v->dims[k])) FAIL("Index out of bounds: %.17g", ix);
    off = off * v->dims[k] + (size_t)ix;
  }
  return &v->cells[off];
}

/* Basic.Variable: the storage cell for an assignable name with its subscripts. */
static double *lvalue_cell(State *st, const char *lname, const double *args, int nargs) {
  if (nargs == 0) {
    double *cell;
    if (frame_lookup(st, lname, &cell)) return cell;
  }
  Var *v = get_value(st, lname, nargs);
  switch (v->kind) {
  case V_SCALAR:
    if (nargs != 0) FAIL("Should not pass arguments to simple numeric variables");
    return &v->value;
  case V_ARRAY:
    return array_cell(st, v, args, nargs);
  default:
    FAIL("Cannot use a function as an lvalue");
  }
  return NULL;
}

static double call_native(State *st, int id, const double *a) {
  switch (id) {
  case NAT_ABS: return fabs(a[0]);
  case NAT_ATN: return atan(a[0]);
  case NAT_COS: return cos(a[0]);
  case NAT_EXP: return exp(a[0]);
  case NAT_INT: return floor(a[0]);
  case NAT_LOG: return log(a[0]);
  case NAT_RND: return rng_next(st);
  case NAT_SGN: return js_sign(a[0]);
  case NAT_SIN: return sin(a[0]);
  case NAT_SQR: return sqrt(a[0]);
  case NAT_TAN: return tan(a[0]);
  }
  return NAN;
}

static double eval_num(State *st, const Expr *e);

/* Basic.NumberApply: a function call, an array read, or a scalar read. */
static double eval_apply(State *st, const Expr *e) {
  double args[MAX_ARGS];
  for (int i = 0; i < e->nargs; i++) args[i] = eval_num(st, e->args[i]);
  if (e->nargs == 0) {
    double *cell;
    if (frame_lookup(st, e->lname, &cell)) return *cell;
  }
  Var *v = get_value(st, e->lname, e->nargs);
  switch (v->kind) {
  case V_SCALAR:
    if (e->nargs != 0) FAIL("Should not pass arguments to simple numeric variables");
    return v->value;
  case V_ARRAY:
    return *array_cell(st, v, args, e->nargs);
  case V_NATIVE:
    if (v->arity != e->nargs)
      FAIL("Expected %d arguments but %d were passed", v->arity, e->nargs);
    return call_native(st, v->native, args);
  default: { /* V_FUNC: NumberFunction.apply binds the parameters in a new frame */
    if (v->nparams != e->nargs) FAIL("Expected %d arguments but %d were passed", v->nparams, e->nargs);
    Frame f;
    f.names = v->params;
    f.values = args;
    f.n = e->nargs;
    f.up = st->frame;
    st->frame = &f;
    double result = eval_num(st, v->body);
    st->frame = f.up;
    return result;
  }
  }
}

typedef struct { const char *s; size_t n; } Str;

static Str eval_str(State *st, const Expr *e) {
  if (e->kind == EX_STR_CONST) {
    Str r = {e->str, e->slen};
    return r;
  }
  if (e->kind == EX_STR_VAR) FAIL("Could not find string variable %s", e->name);
  FAIL("expected a string expression");
  Str none = {"", 0};
  return none;
}

static double eval_num(State *st, const Expr *e) {
  switch (e->kind) {
  case EX_NUM_CONST: return e->value;
  case EX_NUM_APPLY: return eval_apply(st, e);
  case EX_VARIABLE: {
    double args[MAX_ARGS];
    for (int i = 0; i < e->nargs; i++) args[i] = eval_num(st, e->args[i]);
    return *lvalue_cell(st, e->lname, args, e->nargs);
  }
  case EX_POW: {
    double l = eval_num(st, e->left);
    double r = eval_num(st, e->right);
    return pow(l, r);
  }
  case EX_MUL: {
    double l = eval_num(st, e->left);
    return l * eval_num(st, e->right);
  }
  case EX_DIV: {
    double l = eval_num(st, e->left);
    return l / eval_num(st, e->right);
  }
  case EX_NEG: return -eval_num(st, e->left);
  case EX_ADD: {
    double l = eval_num(st, e->left);
    return l + eval_num(st, e->right);
  }
  case EX_SUB: {
    double l = eval_num(st, e->left);
    return l - eval_num(st, e->right);
  }
  case EX_EQ: {
    double l = eval_num(st, e->left);
    return l == eval_num(st, e->right);
  }
  case EX_NE: {
    double l = eval_num(st, e->left);
    return l != eval_num(st, e->right);
  }
  case EX_LT: {
    double l = eval_num(st, e->left);
    return l < eval_num(st, e->right);
  }
  case EX_GT: {
    double l = eval_num(st, e->left);
    return l > eval_num(st, e->right);
  }
  case EX_LE: {
    double l = eval_num(st, e->left);
    return l <= eval_num(st, e->right);
  }
  case EX_GE: {
    double l = eval_num(st, e->left);
    return l >= eval_num(st, e->right);
  }
  case EX_STR_EQ:
  case EX_STR_NE: {
    Str l = eval_str(st, e->left);
    Str r = eval_str(st, e->right);
    int same = l.n == r.n && memcmp(l.s, r.s, l.n) == 0;
    return (e->kind == EX_STR_EQ) == same;
  }
  default:
    FAIL("expected a numeric expression");
  }
  return 0;
}


static int should_stop(const Stmt *f) {
  return (*f->for_var - f->for_limit) * js_sign(f->for_step) > 0;
}

static void do_print(State *st, const Stmt *s) {
  Buf line = {NULL, 0, 0};
  for (int i = 0; i < s->nitems; i++) {
    const PrintItem *it = &s->items[i];
    switch (it->kind) {
    case PI_COMMA:
      while (line.len % 14) buf_putc(&line, ' ');
      break;
    case PI_TAB: {
      double v = js_max(js_round(eval_num(st, it->value)), 1);
      while (truthy(fmod((double)line.len, v))) buf_putc(&line, ' ');
      break;
    }
    case PI_STRING: {
      Str v = eval_str(st, it->value);
      if (v.n) buf_append(&line, v.s, v.n);
      break;
    }
    default: {
      char tmp[40];
      size_t n = js_number_to_string(eval_num(st, it->value), tmp);
      buf_append(&line, tmp, n);
      break;
    }
    }
  }
  if (line.len) buf_append(&st->out, line.data, line.len);
  buf_putc(&st->out, '\n');
  buf_free(&line);
}

static void exec_statement(State *st, Stmt *s) {
  switch (s->kind) {
  case SK_DEF: {
    if (find_global(st, s->lname)) FAIL("Cannot redefine function");
    Var *v = calloc(1, sizeof *v);
    if (!v) FAIL("out of memory");
    v->kind = V_FUNC;
    v->params = s->params;
    v->nparams = s->nparams;
    v->body = s->expr;
    add_global(st, s->lname, v);
    break;
  }
  case SK_LET: {
    double args[MAX_ARGS];
    for (int i = 0; i < s->variable->nargs; i++) args[i] = eval_num(st, s->variable->args[i]);
    double *cell = lvalue_cell(st, s->variable->lname, args, s->variable->nargs);
    *cell = eval_num(st, s->expr);
    break;
  }
  case SK_GOTO:
    st->next_line = s->target;
    break;
  case SK_GOSUB:
    if (st->nsub == st->capsub) {
      size_t nc = st->capsub ? st->capsub * 2 : 16;
      long *a = realloc(st->substack, nc * sizeof *a);
      if (!a) FAIL("out of memory");
      st->substack = a;
      st->capsub = nc;
    }
    st->substack[st->nsub++] = st->next_line;
    st->next_line = s->target;
    break;
  case SK_IF:
    if (truthy(eval_num(st, s->condition))) st->next_line = s->target;
    break;
  case SK_RETURN:
    if (st->nsub == 0) FAIL("Not in a subroutine");
    st->next_line = st->substack[--st->nsub];
    break;
  case SK_STOP:
  case SK_END:
    st->has_next = 0;
    break;
  case SK_ON: {
    double index = eval_num(st, s->expr);
    if (!(index >= 1) || !(index <= s->ntargets)) FAIL("Index out of bounds: %.17g", index);
    st->next_line = s->targets[(long)floor(index) - 1];
    break;
  }
  case SK_FOR: {
    double *var = lvalue_cell(st, s->lname, NULL, 0);
    double initial = eval_num(st, s->initial);
    s->for_limit = eval_num(st, s->limit);
    s->for_step = eval_num(st, s->step);
    *var = initial;
    s->for_var = var;
    if (should_stop(s)) st->next_line = s->loop_next->line_number + 1;
    break;
  }
  case SK_NEXT: {
    Stmt *f = s->loop_for;
    *f->for_var = *f->for_var + f->for_step;
    if (should_stop(f)) break;
    st->next_line = f->line_number + 1;
    break;
  }
  case SK_PRINT:
    do_print(st, s);
    break;
  case SK_INPUT:
    /* The harness passes no input values, so the JS validation fails here as well. */
    FAIL("At %d: Input did not get the right number of items", s->source_line);
    break;
  case SK_READ:
    for (int i = 0; i < s->nvars; i++) {
      Expr *v = s->vars[i];
      if (st->data_index >= st->prog->ndata) FAIL("Attempting to read past the end of data");
      Expr *d = st->prog->data[st->data_index++];
      if (d->kind != EX_NUM_CONST) FAIL("READ of a string into a numeric variable is not supported");
      double args[MAX_ARGS];
      for (int k = 0; k < v->nargs; k++) args[k] = eval_num(st, v->args[k]);
      *lvalue_cell(st, v->lname, args, v->nargs) = d->value;
    }
    break;
  case SK_RESTORE:
    st->data_index = 0;
    break;
  case SK_DIM:
    for (int i = 0; i < s->ndims; i++) {
      DimItem *d = &s->dims[i];
      if (find_global(st, d->lname)) FAIL("Variable %s already exists", d->name);
      Var *v = calloc(1, sizeof *v);
      if (!v) FAIL("out of memory");
      v->kind = V_ARRAY;
      v->ndims = d->nbounds;
      size_t total = 1;
      for (int k = 0; k < d->nbounds; k++) {
        v->dims[k] = (size_t)d->bounds[k] + 1;
        total *= v->dims[k];
      }
      v->cells = calloc(total, sizeof(double));
      if (!v->cells) FAIL("out of memory");
      v->ncells = total;
      add_global(st, d->lname, v);
    }
    break;
  case SK_RANDOMIZE: {
    struct timespec ts;
    timespec_get(&ts, TIME_UTC);
    st->rng = (uint32_t)ts.tv_nsec ^ (uint32_t)ts.tv_sec;
    break;
  }
  default:
    break;
  }
}

/* Basic.Program: runs statements from line 0 upward, one line number at a time. */
static void run_program(State *st) {
  Program *p = st->prog;
  while (st->has_next) {
    if (st->next_line > p->max_line) FAIL("Went out of bounds of the program");
    Stmt *s = lookup_statement(p, st->next_line);
    st->next_line++;
    if (!s || s->kind == SK_NONE) continue;
    exec_statement(st, s);
  }
}

/* ---------- harness (benchmark.js) ---------- */

static int expect_count;

static void expect(const char *program, const char *expected) {
  int index = ++expect_count;
  Program *p = parse_program(program);
  State st;
  state_init(&st, p);
  run_program(&st);
  const char *got = st.out.data ? st.out.data : "";
  if (strcmp(got, expected) != 0) {
    fprintf(stderr, "Basic: program %d produced %zu bytes but we expected %zu\n", index, st.out.len, strlen(expected));
    exit(1);
  }
  state_release(&st);
  free_program(p);
  arena_release();
}

static void run_iteration(void) {
        expect("10 print \"hello, world!\"\n20 end", "hello, world!\n");
        expect("10 let x = 0\n20 let x = x + 1\n30 print x\n40 if x < 10 then 20\n50 end", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
        expect("10 print int(rnd * 100)\n20 end\n", "98\n");
        expect("10 let value = int(rnd * 2000)\n20 print value\n30 if value <> 100 then 10\n40 end", "1974\n697\n1126\n1998\n1658\n264\n1650\n1677\n226\n117\n492\n861\n877\n1969\n38\n1039\n197\n1261\n"
    "1102\n1522\n916\n1683\n1943\n1835\n476\n1898\n939\n176\n966\n908\n474\n614\n1326\n564\n1916\n728\n524\n162\n1303\n758\n832\n1279\n1856\n1876\n982\n6\n1613\n1781\n681\n1238\n494\n1583\n1953\n788\n1026\n347\n1116\n1465\n"
    "514\n583\n463\n1970\n1573\n412\n1256\n1453\n838\n1538\n1984\n1598\n209\n411\n1700\n546\n861\n91\n132\n884\n378\n693\n11\n433\n1719\n860\n164\n472\n231\n1786\n806\n811\n106\n1697\n118\n980\n890\n1199\n227\n1667\n"
    "1933\n1903\n1390\n1595\n923\n1746\n39\n1361\n117\n1297\n923\n901\n1180\n818\n1444\n269\n933\n327\n1744\n1082\n1527\n1260\n622\n528\n318\n856\n296\n1796\n1574\n585\n1871\n111\n827\n1725\n1320\n1868\n1695\n1914\n216\n63\n"
    "1847\n156\n671\n893\n127\n1867\n811\n279\n913\n310\n814\n907\n1363\n1624\n1670\n478\n714\n436\n355\n1484\n1628\n1208\n800\n611\n917\n829\n830\n273\n1791\n340\n214\n992\n1444\n442\n1555\n144\n1194\n282\n180\n1228\n"
    "1251\n1883\n678\n1555\n347\n72\n1661\n1828\n1090\n1183\n957\n1685\n930\n475\n103\n759\n1725\n1902\n1662\n1587\n61\n614\n863\n1418\n321\n1050\n505\n1622\n1425\n803\n589\n1511\n1098\n1051\n1554\n1898\n27\n747\n813\n1544\n"
    "332\n728\n1363\n771\n759\n1145\n1098\n1991\n385\n230\n520\n1369\n1840\n1285\n1562\n1845\n102\n760\n1874\n748\n361\n575\n277\n1661\n1764\n1117\n332\n757\n1766\n1722\n143\n474\n1507\n1294\n1180\n1578\n904\n845\n321\n496\n"
    "1911\n1784\n1116\n938\n1591\n1403\n1374\n533\n1085\n452\n708\n1096\n1634\n522\n564\n1397\n1357\n980\n978\n1760\n1088\n1361\n1184\n314\n1242\n217\n133\n1187\n1723\n646\n605\n591\n46\n135\n1420\n1821\n1147\n1211\n61\n244\n"
    "1307\n1551\n449\n1122\n1336\n140\n880\n22\n1155\n1326\n590\n1499\n1376\n112\n1771\n1897\n1071\n938\n1685\n1963\n1203\n1296\n804\n1275\n453\n1387\n482\n1262\n1883\n1381\n418\n1417\n1222\n1208\n1263\n632\n450\n1422\n1285\n1408\n"
    "644\n665\n275\n363\n1012\n165\n354\n80\n609\n291\n1661\n1724\n117\n407\n59\n906\n1224\n136\n855\n1275\n1468\n482\n1537\n1283\n1784\n1568\n1832\n452\n867\n1546\n1467\n800\n45\n1225\n1890\n465\n1372\n47\n1608\n193\n"
    "1345\n1847\n1059\n1788\n518\n52\n1052\n1003\n1210\n1135\n1433\n519\n1558\n39\n1249\n1017\n39\n1713\n1449\n1245\n1354\n82\n1140\n916\n1595\n838\n607\n389\n1270\n821\n247\n1692\n1305\n1211\n1960\n429\n1703\n1635\n575\n1618\n"
    "1490\n1495\n682\n1256\n964\n420\n1520\n1429\n1997\n396\n382\n856\n1182\n296\n1295\n298\n1892\n990\n711\n934\n1939\n1339\n682\n1631\n1533\n742\n1520\n1281\n1332\n1042\n656\n1576\n1253\n1608\n375\n169\n14\n414\n1586\n1562\n"
    "1508\n1245\n303\n715\n1053\n340\n915\n160\n1796\n111\n925\n1872\n735\n350\n107\n1913\n1653\n987\n825\n1893\n1601\n460\n1228\n1526\n1613\n1359\n1854\n1352\n542\n665\n109\n1874\n467\n533\n1188\n1629\n851\n630\n1060\n1530\n"
    "1853\n743\n765\n126\n1540\n1411\n858\n1741\n284\n299\n577\n1848\n1495\n283\n1886\n284\n129\n1077\n1245\n1364\n1505\n176\n1012\n1663\n1306\n1586\n410\n315\n660\n256\n1102\n1289\n1292\n939\n762\n601\n1140\n574\n1851\n44\n"
    "560\n1948\n1142\n1787\n947\n948\n280\n1210\n1139\n1072\n1033\n92\n1244\n1589\n1079\n22\n1514\n163\n157\n1742\n1058\n514\n196\n1858\n565\n354\n1413\n792\n183\n526\n1724\n1007\n158\n1229\n1802\n99\n1514\n708\n1276\n1802\n"
    "1564\n1387\n1235\n1132\n715\n1584\n617\n1664\n1559\n1625\n1037\n601\n1175\n1713\n107\n88\n384\n1634\n904\n1835\n1472\n212\n1145\n443\n1617\n866\n1963\n937\n1917\n855\n1215\n1867\n520\n892\n1483\n1898\n1747\n1441\n289\n1609\n"
    "328\n566\n271\n458\n1616\n843\n1107\n507\n1090\n854\n1094\n806\n166\n408\n661\n334\n230\n1917\n1323\n927\n1912\n673\n311\n952\n1783\n1549\n1714\n1500\n450\n1498\n530\n442\n607\n609\n1226\n370\n1769\n1815\n788\n536\n"
    "293\n115\n947\n290\n1764\n243\n1219\n1851\n289\n599\n1528\n150\n1859\n297\n279\n1542\n1719\n1910\n551\n401\n952\n1764\n946\n1835\n647\n1309\n271\n275\n70\n129\n1518\n972\n1164\n816\n1125\n575\n588\n1456\n1154\n290\n"
    "1681\n1133\n561\n343\n1360\n1035\n1158\n1365\n744\n781\n58\n531\n271\n1612\n1774\n28\n1480\n1312\n1855\n666\n1574\n613\n42\n456\n351\n727\n1503\n1115\n333\n1972\n822\n1575\n848\n1087\n1262\n1671\n710\n460\n1816\n287\n"
    "172\n492\n1079\n582\n1236\n1756\n1792\n1095\n1205\n1894\n22\n1930\n1529\n1547\n1383\n1768\n364\n1108\n1972\n287\n200\n230\n1335\n187\n486\n1722\n20\n963\n792\n1114\n633\n1862\n1433\n829\n737\n215\n1570\n378\n1677\n944\n"
    "1301\n1160\n500\n150\n886\n1337\n662\n1062\n290\n460\n592\n1867\n872\n155\n1613\n1913\n1548\n1847\n855\n1702\n952\n1894\n587\n1813\n1021\n21\n654\n254\n910\n1696\n1606\n679\n1222\n696\n1319\n368\n447\n549\n905\n1194\n"
    "189\n1766\n616\n278\n1418\n1965\n872\n998\n1268\n1673\n1647\n1163\n533\n1650\n1849\n1124\n1252\n1412\n703\n944\n468\n1485\n1352\n681\n864\n1432\n1771\n497\n956\n1794\n363\n1099\n1804\n457\n1227\n1487\n446\n1993\n1576\n272\n"
    "709\n1810\n330\n876\n1107\n1187\n122\n1625\n472\n676\n314\n1257\n1509\n350\n741\n366\n33\n536\n293\n1663\n1039\n1527\n126\n923\n1937\n1767\n1302\n1510\n1518\n1343\n91\n1551\n1614\n1687\n1748\n137\n75\n738\n1977\n751\n"
    "237\n313\n566\n24\n202\n889\n1716\n1460\n129\n1760\n1597\n96\n1057\n1323\n1188\n1373\n537\n955\n65\n1679\n1441\n1315\n398\n647\n1470\n1335\n617\n331\n796\n129\n1635\n1497\n836\n855\n1472\n1828\n568\n862\n690\n1370\n"
    "1657\n819\n45\n420\n258\n1980\n672\n615\n358\n852\n1148\n1897\n1306\n1092\n1405\n719\n1752\n1456\n1338\n332\n351\n479\n747\n249\n1977\n1671\n1061\n1685\n306\n254\n1060\n764\n420\n1139\n1452\n426\n835\n929\n1424\n1336\n"
    "697\n191\n1697\n1897\n644\n546\n982\n359\n1201\n1095\n1623\n1947\n215\n10\n855\n297\n551\n1037\n945\n396\n211\n1059\n423\n1521\n1770\n203\n1828\n879\n1179\n1912\n1028\n1416\n1845\n698\n715\n1857\n817\n50\n473\n1122\n"
    "126\n70\n1773\n40\n1970\n1311\n826\n355\n1921\n23\n526\n1717\n1397\n1932\n1075\n1652\n997\n1039\n1481\n779\n415\n49\n1330\n317\n1701\n690\n245\n1824\n639\n799\n1240\n422\n344\n1639\n20\n546\n912\n1930\n1368\n1541\n"
    "1109\n369\n66\n1564\n444\n1928\n1963\n1899\n744\n1593\n1702\n100\n");
        expect("10 dim a(2000)\n20 for i = 2 to 2000\n30 let a(i) = 1\n40 next i\n50 for i = 2 to sqr(2000)\n60 if a(i) = 0 then 100\n70 for j = i ^ 2 to 2000 step i\n80 let a(j) = 0\n90 next j\n100 next i\n110 for i = 2 to 2000\n120 if a(i) = 0 then 140\n130 print i\n140 next i\n150 end\n", "2\n3\n5\n7\n11\n13\n17\n19\n23\n29\n31\n37\n41\n"
    "43\n47\n53\n59\n61\n67\n71\n73\n79\n83\n89\n97\n101\n103\n107\n109\n113\n127\n131\n137\n139\n149\n151\n157\n163\n167\n173\n179\n181\n191\n193\n197\n199\n211\n223\n227\n229\n233\n239\n241\n"
    "251\n257\n263\n269\n271\n277\n281\n283\n293\n307\n311\n313\n317\n331\n337\n347\n349\n353\n359\n367\n373\n379\n383\n389\n397\n401\n409\n419\n421\n431\n433\n439\n443\n449\n457\n461\n463\n467\n479\n487\n"
    "491\n499\n503\n509\n521\n523\n541\n547\n557\n563\n569\n571\n577\n587\n593\n599\n601\n607\n613\n617\n619\n631\n641\n643\n647\n653\n659\n661\n673\n677\n683\n691\n701\n709\n719\n727\n733\n739\n743\n751\n"
    "757\n761\n769\n773\n787\n797\n809\n811\n821\n823\n827\n829\n839\n853\n857\n859\n863\n877\n881\n883\n887\n907\n911\n919\n929\n937\n941\n947\n953\n967\n971\n977\n983\n991\n997\n1009\n1013\n1019\n1021\n1031\n"
    "1033\n1039\n1049\n1051\n1061\n1063\n1069\n1087\n1091\n1093\n1097\n1103\n1109\n1117\n1123\n1129\n1151\n1153\n1163\n1171\n1181\n1187\n1193\n1201\n1213\n1217\n1223\n1229\n1231\n1237\n1249\n1259\n1277\n1279\n1283\n1289\n1291\n1297\n1301\n1303\n"
    "1307\n1319\n1321\n1327\n1361\n1367\n1373\n1381\n1399\n1409\n1423\n1427\n1429\n1433\n1439\n1447\n1451\n1453\n1459\n1471\n1481\n1483\n1487\n1489\n1493\n1499\n1511\n1523\n1531\n1543\n1549\n1553\n1559\n1567\n1571\n1579\n1583\n1597\n1601\n1607\n"
    "1609\n1613\n1619\n1621\n1627\n1637\n1657\n1663\n1667\n1669\n1693\n1697\n1699\n1709\n1721\n1723\n1733\n1741\n1747\n1753\n1759\n1777\n1783\n1787\n1789\n1801\n1811\n1823\n1831\n1847\n1861\n1867\n1871\n1873\n1877\n1879\n1889\n1901\n1907\n1913\n"
    "1931\n1933\n1949\n1951\n1973\n1979\n1987\n1993\n1997\n1999\n");
}

static double now_ms(void) {
  struct timespec ts;
  timespec_get(&ts, TIME_UTC);
  return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  double t0 = now_ms();
  for (int i = 0; i < iterations; i++) run_iteration();
  double t1 = now_ms();
  printf("Basic %d iterations %.3f ms\n", iterations, t1 - t0);
  return 0;
}
