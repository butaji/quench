// jetstream-row: WSL
// jetstream-files: WSL/OriginKind.js WSL/WSyntaxError.js WSL/LexerToken.js WSL/Lexer.js WSL/StandardLibrary.js WSL/AddressSpace.js WSL/Test.js
// iterations: 120
/* C analogue of the lexing stage of JetStream's WSL row (WSL/Lexer.js, LexerToken.js).
 *
 * Scope: this is the lexer only. The JS row compiles every test program through the full
 * pipeline (Parse.js, the name resolver, Checker.js and the rest of the WSL passes, about
 * 20k lines in 160 files). This analogue lexes the same program texts: the standard library
 * (native origin) and every literal argument of doPrep(...) (user origin) or doLex(...)
 * (native origin) in WSL/Test.js; the one template with ${addressSpace} expands over
 * addressSpaces from WSL/AddressSpace.js, as the JS does. Each iteration lexes all inputs;
 * the lexer follows Lexer.next() step by step, including its leftmost-search punctuation
 * rule (only the first alternative of that regex is anchored).
 * Validation: input count, character count, token count, error count and the sum of a
 * per-input FNV-1a over (kind, text) must match the JS lexer's results on the same inputs.
 * Usage: wsl [jetstream-root] [iterations]   root defaults to ../jetstream; iterations to 120. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

/* ---- lexer (Lexer.js next()) ---- */
typedef struct { const char *text; int len; int index; int native; } Lexer;
typedef struct { const char *kind; const char *text; int len; } Token;

static int is_js_space(int c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f';
}
static int is_lt(int c) { return c == '\n' || c == '\r'; }
static int is_digit(int c) { return c >= '0' && c <= '9'; }
static int is_hex(int c) { return is_digit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
static int is_word(int c) { return is_digit(c) || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_'; }

/* float: digits '.' digits, or digits '.' digits*, with an optional f or d suffix */
static int match_float(const char *r, int n) {
  int j = 0;
  while (j < n && is_digit((unsigned char)r[j])) j++;
  if (j < n && r[j] == '.' && j + 1 < n && is_digit((unsigned char)r[j + 1])) {
    j++;
    while (j < n && is_digit((unsigned char)r[j])) j++;
    if (j < n && (r[j] == 'f' || r[j] == 'd')) j++;
    return j;
  }
  j = 0;
  while (j < n && is_digit((unsigned char)r[j])) j++;
  if (j == 0) return 0;
  if (j < n && r[j] == '.') {
    j++;
    while (j < n && is_digit((unsigned char)r[j])) j++;
    if (j < n && (r[j] == 'f' || r[j] == 'd')) j++;
    return j;
  }
  return 0;
}
/* identifier: first char a letter or underscore, then word characters */
static int match_ident(const char *r, int n) {
  if (n == 0 || is_digit((unsigned char)r[0]) || !is_word((unsigned char)r[0])) return 0;
  int j = 1;
  while (j < n && is_word((unsigned char)r[j])) j++;
  return j;
}
static int is_keyword(const char *s, int n) {
  static const char *kw[] = {"struct", "protocol", "typedef", "if", "else", "enum", "continue", "break",
                             "switch", "case", "default", "for", "while", "do", "return", "constant",
                             "device", "threadgroup", "thread", "operator", "null", "true", "false", NULL};
  for (int i = 0; kw[i]; i++) if ((int)strlen(kw[i]) == n && memcmp(kw[i], s, n) == 0) return 1;
  return 0;
}
/* The punctuation alternation; the first alternative (<<) only matches at index 0. */
static const char *PUNCT[] = {">>", "->", ">=", "<=", "==", "!=", "+=", "-=", "*=", "/=", "%=", "^=",
                              "|=", "&=", "++", "--", "&&", "||", NULL};
static const char SINGLE[] = "{}()[]?:=+*/,.%!~^&|<>@;-";
static int match_punct(const char *r, int n) {
  for (int p = 0; p < n; p++) {
    if (p == 0 && n >= 2 && r[0] == '<' && r[1] == '<') return 2;
    for (int a = 0; PUNCT[a]; a++) {
      if (n - p >= 2 && r[p] == PUNCT[a][0] && r[p + 1] == PUNCT[a][1]) return 2;
    }
    if (strchr(SINGLE, r[p]) && r[p]) return 1;
  }
  return 0;
}

/* Lexer.next(): 1 = token, 0 = end (null), -1 = syntax error. */
static int lex_next(Lexer *L, Token *out) {
  if (L->index >= L->len) return 0;
  for (;;) {
    const char *r = L->text + L->index;
    int n = L->len - L->index;
    if (n > 0 && is_js_space((unsigned char)r[0])) {
      int k = 0;
      while (k < n && is_js_space((unsigned char)r[k])) k++;
      L->index += k;
      continue;
    }
    if (n >= 2 && r[0] == '/' && r[1] == '*') {
      int e = -1;
      for (int k = 0; k + 1 < n; k++) if (r[k] == '*' && r[k + 1] == '/') { e = k; break; }
      if (e < 0) return -1;
      L->index += e;
      continue;
    }
    if (n >= 2 && r[0] == '/' && r[1] == '/') {
      int k = 2;
      while (k < n && !is_lt((unsigned char)r[k])) k++;
      L->index += k;
      continue;
    }
    break;
  }
  if (L->index >= L->len) return 0;
  const char *r = L->text + L->index;
  int n = L->len - L->index;
  int m;
  if ((m = match_float(r, n))) { out->kind = "floatLiteral"; out->text = r; out->len = m; goto done; }
  if ((m = match_ident(r, n))) {
    out->text = r; out->len = m;
    if (is_keyword(r, m)) out->kind = "keyword";
    else if (L->native && ((m == 6 && !memcmp(r, "native", 6)) || (m == 10 && !memcmp(r, "restricted", 10)))) out->kind = "keyword";
    else out->kind = "identifier";
    goto done;
  }
  if (n >= 2 && r[0] == '0' && r[1] == 'x') {
    int j = 2;
    while (j < n && is_hex((unsigned char)r[j])) j++;
    if (j > 2) {
      if (j < n && r[j] == 'u') { out->kind = "uintHexLiteral"; out->text = r; out->len = j + 1; goto done; }
      out->kind = "intHexLiteral"; out->text = r; out->len = j; goto done;
    }
  }
  {
    int j = 0;
    while (j < n && is_digit((unsigned char)r[j])) j++;
    if (j > 0) {
      if (j < n && r[j] == 'u') { out->kind = "uintLiteral"; out->text = r; out->len = j + 1; goto done; }
      out->kind = "intLiteral"; out->text = r; out->len = j; goto done;
    }
  }
  if ((m = match_punct(r, n))) { out->kind = "punctuation"; out->text = r; out->len = m; goto done; }
  return -1;
done:
  L->index += out->len;
  return 1;
}

/* ---- workload: the inputs of WSL/Test.js and WSL/StandardLibrary.js ---- */
typedef struct { int native; char *text; int len; } Input;
static Input *INPUTS;
static int NINPUTS, CAPINPUTS;

static void add_input(int native, const char *s, int n) {
  if (NINPUTS == CAPINPUTS) { CAPINPUTS = CAPINPUTS ? CAPINPUTS * 2 : 512; INPUTS = realloc(INPUTS, CAPINPUTS * sizeof *INPUTS); }
  char *c = malloc(n + 1);
  memcpy(c, s, n);
  c[n] = 0;
  INPUTS[NINPUTS].native = native; INPUTS[NINPUTS].text = c; INPUTS[NINPUTS].len = n;
  NINPUTS++;
}
static char *read_file(const char *path, long *len) {
  FILE *f = fopen(path, "rb");
  if (!f) { perror(path); exit(2); }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *b = malloc(n + 1);
  if (fread(b, 1, n, f) != (size_t)n) { fprintf(stderr, "read failed: %s\n", path); exit(2); }
  b[n] = 0;
  fclose(f);
  *len = n;
  return b;
}
/* Single or double quoted literal starting at src[i]; returns the index after it. */
static int decode_quoted(const char *src, long n, long i, char **out, int *outlen) {
  char q = src[i];
  char *buf = malloc(n - i + 1);
  int k = 0;
  for (long j = i + 1; j < n; j++) {
    char c = src[j];
    if (c == q) { buf[k] = 0; *out = buf; *outlen = k; return (int)(j + 1); }
    if (c == '\\' && j + 1 < n) {
      char d = src[++j];
      buf[k++] = d == 'n' ? '\n' : d == 't' ? '\t' : d;
      continue;
    }
    buf[k++] = c;
  }
  fprintf(stderr, "unterminated literal\n");
  exit(3);
}
static const char *ADDRESS_SPACES[] = {"constant", "device", "threadgroup", "thread"};
/* Template literal at src[i] == '`': one input, or one per address space when it has ${addressSpace}. */
static int decode_template(const char *src, long n, long i, int native) {
  long parts_cap = 0, nparts = 1;
  char *parts[8] = {0};
  int plen[8] = {0};
  int bufcap = 1 << 16;
  parts[0] = malloc(bufcap); plen[0] = 0;
  long j = i + 1;
  for (;;) {
    if (j >= n) { fprintf(stderr, "unterminated template\n"); exit(3); }
    char c = src[j];
    if (c == '`') break;
    if (c == '\\' && j + 1 < n) {
      char d = src[++j];
      parts[nparts - 1][plen[nparts - 1]++] = d == 'n' ? '\n' : d == 't' ? '\t' : d;
      j++; continue;
    }
    if (c == '$' && j + 1 < n && src[j + 1] == '{') {
      long close = j + 2;
      while (close < n && src[close] != '}') close++;
      if (close - (j + 2) != 12 || memcmp(src + j + 2, "addressSpace", 12) != 0) { fprintf(stderr, "unknown interpolation\n"); exit(3); }
      parts[nparts] = malloc(bufcap); plen[nparts] = 0; nparts++;
      j = close + 1; continue;
    }
    parts[nparts - 1][plen[nparts - 1]++] = c; j++;
  }
  long end = j + 1;
  if (nparts == 1) {
    add_input(native, parts[0], plen[0]);
  } else {
    for (int a = 0; a < 4; a++) {
      char *s = malloc(bufcap * 8);
      int k = 0;
      for (long p = 0; p < nparts; p++) {
        memcpy(s + k, parts[p], plen[p]); k += plen[p];
        if (p < nparts - 1) { int l = (int)strlen(ADDRESS_SPACES[a]); memcpy(s + k, ADDRESS_SPACES[a], l); k += l; }
      }
      add_input(native, s, k);
      free(s);
    }
  }
  for (long p = 0; p < nparts; p++) free(parts[p]);
  (void)parts_cap;
  return (int)end;
}
static void extract_test_inputs(const char *test, long n) {
  static const char *names[] = {"doPrep(", "doLex("};
  for (long i = 0; i < n; i++) {
    for (int w = 0; w < 2; w++) {
      size_t L = strlen(names[w]);
      if (i + (long)L > n || memcmp(test + i, names[w], L) != 0) continue;
      if (i > 0 && (is_word((unsigned char)test[i - 1]) || test[i - 1] == '$')) continue;
      long j = i + L;
      while (j < n && is_js_space((unsigned char)test[j])) j++;
      if (j >= n) continue;
      int native = w == 1;
      if (test[j] == '"' || test[j] == '\'') {
        char *s; int sl;
        decode_quoted(test, n, j, &s, &sl);
        add_input(native, s, sl);
        free(s);
      } else if (test[j] == '`') {
        decode_template(test, n, j, native);
      }
      i = j - 1;
      break;
    }
  }
}

/* Folds one token into an FNV-1a state: kind, 0x01, text, 0x02. */
static uint32_t fnv_token(uint32_t h, const Token *t) {
  const char *k = t->kind;
  for (; *k; k++) { h ^= (unsigned char)*k; h *= 16777619u; }
  h ^= 1; h *= 16777619u;
  for (int i = 0; i < t->len; i++) { h ^= (unsigned char)t->text[i]; h *= 16777619u; }
  h ^= 2; h *= 16777619u;
  return h;
}

int main(int argc, char **argv) {
  const char *root = argc > 1 ? argv[1] : "../jetstream";
  int iterations = argc > 2 ? atoi(argv[2]) : 120;
  char path[4096];
  long n;
  snprintf(path, sizeof path, "%s/WSL/StandardLibrary.js", root);
  char *std = read_file(path, &n);
  const char *lib = strstr(std, "let standardLibrary = `");
  if (!lib) { fprintf(stderr, "standard library not found\n"); return 2; }
  decode_template(lib + strlen("let standardLibrary = "), n - (lib - std) - strlen("let standardLibrary = "), 0, 1);
  /* decode_template above expects the backtick at index 0 of its window, which lib + 22 is */
  free(std);
  snprintf(path, sizeof path, "%s/WSL/Test.js", root);
  char *test = read_file(path, &n);
  extract_test_inputs(test, n);
  free(test);
  /* the address space list is fixed in AddressSpace.js; check the file exists so the root is right */
  snprintf(path, sizeof path, "%s/WSL/AddressSpace.js", root);
  free(read_file(path, &n));

  long chars = 0;
  for (int i = 0; i < NINPUTS; i++) chars += INPUTS[i].len;
  long tokens = 0, errors = 0;
  uint32_t hashsum = 0;
  double elapsed = 0;
  for (int it = 0; it < iterations; it++) {
    double t0 = now_ms();
    long tok = 0, err = 0;
    uint32_t hs = 0;
    for (int i = 0; i < NINPUTS; i++) {
      Lexer L = {INPUTS[i].text, INPUTS[i].len, 0, INPUTS[i].native};
      Token t;
      uint32_t h = 0x811c9dc5u;
      int r;
      long count = 0;
      while ((r = lex_next(&L, &t)) == 1) {
        count++;
        h = fnv_token(h, &t);
      }
      if (r < 0) err++;
      tok += count;
      hs += h;
    }
    elapsed += now_ms() - t0;
    tokens = tok; errors = err; hashsum = hs;
  }
  printf("wsl lexer %d iterations %.3f ms\n", iterations, elapsed);
  printf("inputs %d chars %ld tokens %ld errors %ld hashsum %u\n", NINPUTS, chars, tokens, errors, (unsigned)hashsum);
  return 0;
}
