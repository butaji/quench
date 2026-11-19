/* WSL lexer, the C form of WSL/Lexer.js next() (shared by wsl.c and wsl-parse.h).
 *
 * Positions and token texts follow Lexer.js exactly, including two quirks of its regexes:
 *   - the punctuation alternation is unanchored apart from its first alternative (<<), so the
 *     match may start after the current position; the token text is that match, but the index
 *     advances by the match length from the current position (match_punct returns both);
 *   - a block comment ends at its "*\/" and the lexer resumes at that "*", so the terminator
 *     is then lexed as punctuation.
 * Errors: lex_next returns -1 and sets errmsg to the Lexer.js fail() message. The parser adds
 * the "Syntax error at origin:line: " prefix at the lexer's current index, as Lexer.fail does. */
#ifndef WSL_LEX_H
#define WSL_LEX_H

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  const char *kind;  /* "identifier", "keyword", "punctuation", "intLiteral", ...; NULL = null token */
  const char *text;  /* not NUL-terminated: use tok_is() or len */
  int len;
  int index;         /* start of the token in the source text */
} Tok;

typedef struct {
  const char *text;
  int len;
  int index;
  int native;        /* originKind == "native" */
  const char *origin;
  int lineOffset;
  const char *errmsg;
} Lexer;

static int lex_is_js_space(int c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f';
}
static int lex_is_lt(int c) { return c == '\n' || c == '\r'; }
static int lex_is_digit(int c) { return c >= '0' && c <= '9'; }
static int lex_is_hex(int c) { return lex_is_digit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
static int lex_is_word(int c) { return lex_is_digit(c) || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_'; }

/* /^(([0-9]*\.[0-9]+[fd]?)|([0-9]+\.[0-9]*[fd]?))/ : match length or 0 */
static int lex_match_float(const char *r, int n) {
  int j = 0;
  while (j < n && lex_is_digit((unsigned char)r[j])) j++;
  if (j < n && r[j] == '.' && j + 1 < n && lex_is_digit((unsigned char)r[j + 1])) {
    j++;
    while (j < n && lex_is_digit((unsigned char)r[j])) j++;
    if (j < n && (r[j] == 'f' || r[j] == 'd')) j++;
    return j;
  }
  j = 0;
  while (j < n && lex_is_digit((unsigned char)r[j])) j++;
  if (j == 0) return 0;
  if (j < n && r[j] == '.') {
    j++;
    while (j < n && lex_is_digit((unsigned char)r[j])) j++;
    if (j < n && (r[j] == 'f' || r[j] == 'd')) j++;
    return j;
  }
  return 0;
}
/* identifier start: a word character that is not a digit, then word characters */
static int lex_match_ident(const char *r, int n) {
  if (n == 0 || lex_is_digit((unsigned char)r[0]) || !lex_is_word((unsigned char)r[0])) return 0;
  int j = 1;
  while (j < n && lex_is_word((unsigned char)r[j])) j++;
  return j;
}
static int lex_is_keyword(const char *s, int n) {
  static const char *kw[] = {"struct", "protocol", "typedef", "if", "else", "enum", "continue", "break",
                             "switch", "case", "default", "for", "while", "do", "return", "constant",
                             "device", "threadgroup", "thread", "operator", "null", "true", "false", NULL};
  for (int i = 0; kw[i]; i++) if ((int)strlen(kw[i]) == n && memcmp(kw[i], s, n) == 0) return 1;
  return 0;
}
static const char *LEX_PUNCT2[] = {">>", "->", ">=", "<=", "==", "!=", "+=", "-=", "*=", "/=", "%=", "^=",
                                   "|=", "&=", "++", "--", "&&", "||", NULL};
static const char LEX_SINGLE[] = "{}()[]?:=+*/,.%!~^&|<>@;-";
/* The punctuation regex's leftmost match. Returns the match length (0 = none) and its start
 * offset p in *start. The first alternative (<<) only matches at offset 0. */
static int lex_match_punct(const char *r, int n, int *start) {
  for (int p = 0; p < n; p++) {
    if (p == 0 && n >= 2 && r[0] == '<' && r[1] == '<') { *start = 0; return 2; }
    for (int a = 0; LEX_PUNCT2[a]; a++) {
      if (n - p >= 2 && r[p] == LEX_PUNCT2[a][0] && r[p + 1] == LEX_PUNCT2[a][1]) { *start = p; return 2; }
    }
    if (r[p] && strchr(LEX_SINGLE, r[p])) { *start = p; return 1; }
  }
  return 0;
}

static Tok lex_make(const char *kind, const char *text, int len, int index) {
  Tok t;
  t.kind = kind; t.text = text; t.len = len; t.index = index;
  return t;
}

/* Lexer.next(): 1 = token written to *out, 0 = end of input (null), -1 = error (L->errmsg). */
static int lex_next(Lexer *L, Tok *out) {
  if (L->index >= L->len) return 0;
  for (;;) {
    const char *r = L->text + L->index;
    int n = L->len - L->index;
    if (n > 0 && lex_is_js_space((unsigned char)r[0])) {
      int k = 0;
      while (k < n && lex_is_js_space((unsigned char)r[k])) k++;
      L->index += k;
      continue;
    }
    if (n >= 2 && r[0] == '/' && r[1] == '*') {
      int e = -1;
      for (int k = 0; k + 1 < n; k++) if (r[k] == '*' && r[k + 1] == '/') { e = k; break; }
      if (e < 0) { L->errmsg = "Unterminated comment"; return -1; }
      L->index += e;
      continue;
    }
    if (n >= 2 && r[0] == '/' && r[1] == '/') {
      int k = 2;
      while (k < n && !lex_is_lt((unsigned char)r[k])) k++;
      L->index += k;
      continue;
    }
    break;
  }
  if (L->index >= L->len) return 0;
  const char *r = L->text + L->index;
  int n = L->len - L->index;
  int m;
  int at = L->index;
  if ((m = lex_match_float(r, n))) { *out = lex_make("floatLiteral", r, m, at); goto done; }
  if ((m = lex_match_ident(r, n))) {
    const char *kind;
    if (lex_is_keyword(r, m)) kind = "keyword";
    else if (L->native && ((m == 6 && !memcmp(r, "native", 6)) || (m == 10 && !memcmp(r, "restricted", 10)))) kind = "keyword";
    else kind = "identifier";
    *out = lex_make(kind, r, m, at);
    goto done;
  }
  if (n >= 2 && r[0] == '0' && r[1] == 'x') {
    int j = 2;
    while (j < n && lex_is_hex((unsigned char)r[j])) j++;
    if (j > 2) {
      if (j < n && r[j] == 'u') { *out = lex_make("uintHexLiteral", r, j + 1, at); goto done; }
      *out = lex_make("intHexLiteral", r, j, at);
      goto done;
    }
  }
  {
    int j = 0;
    while (j < n && lex_is_digit((unsigned char)r[j])) j++;
    if (j > 0) {
      if (j < n && r[j] == 'u') { *out = lex_make("uintLiteral", r, j + 1, at); goto done; }
      *out = lex_make("intLiteral", r, j, at);
      goto done;
    }
  }
  {
    int start = 0;
    if ((m = lex_match_punct(r, n, &start))) {
      *out = lex_make("punctuation", r + start, m, at);
      goto done;
    }
  }
  {
    /* Lexer.js: remaining = relevantText.substring(0, 20).split(/\s/)[0] */
    int k = 0;
    while (k < n && k < 20 && !lex_is_js_space((unsigned char)r[k])) k++;
    static char buf[32];
    memcpy(buf, r, k);
    buf[k] = 0;
    static char msg[96];
    snprintf(msg, sizeof msg, "Unrecognized token beginning with: %s", buf);
    L->errmsg = msg;
    return -1;
  }
done:
  L->index += out->len;
  return 1;
}

/* 1-based line number of a source index (Lexer.lineNumberForIndex + 1) */
__attribute__((unused)) static int lex_line_of(const Lexer *L, int index) {
  int lines = 0;
  for (int i = 0; i < index && i < L->len; i++) if (L->text[i] == '\n') lines++;
  return lines + L->lineOffset + 1;
}

#endif
