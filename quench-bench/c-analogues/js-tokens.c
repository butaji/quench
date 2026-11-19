// jetstream-row: js-tokens
// jetstream-files: generators/js-tokens.js
// iterations: 120
/* C analogue of JetStream's generators/js-tokens.js: the same tokenizer state machine (modes
 * JS, JSX tag, JSX children, template and JSX interpolation, with the same lastSignificantToken
 * rules for telling a regular expression from a division), written as hand-coded matchers for
 * each of the library's sticky regular expressions in the same alternation order. Each run
 * tokenizes the same two inputs as the JS benchmark: the source text of the tokenizer function
 * itself (25 times) and a JSX sample (250 times, with JSX enabled), and counts every token.
 * Inputs are ASCII, so the Unicode ID and whitespace classes reduce to their ASCII members. */
#define _POSIX_C_SOURCE 200809L
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef unsigned char uc;

typedef struct {
  const char *p;
  size_t n;
} Str;

static Str lit(const char *s) {
  Str r;
  r.p = s;
  r.n = strlen(s);
  return r;
}

static Str span(const uc *s, size_t from, size_t to) {
  Str r;
  r.p = (const char *)s + from;
  r.n = to - from;
  return r;
}

static int str_is(Str s, const char *t) {
  size_t m = strlen(t);
  return s.n == m && memcmp(s.p, t, m) == 0;
}

/* ---- character classes (ASCII) ---- */
static int is_digit(uc c) { return c >= '0' && c <= '9'; }
static int is_oct(uc c) { return c >= '0' && c <= '7'; }
static int is_bin(uc c) { return c == '0' || c == '1'; }
static int is_hex(uc c) { return is_digit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
static int is_ident_start(uc c) { return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || c == '$' || c == '_'; }
static int is_ident_part(uc c) { return is_ident_start(c) || is_digit(c); }
static int is_lt(uc c) { return c == '\n' || c == '\r'; }

/* ---- the JS-side tests on lastSignificantToken ---- */
/* TokensPrecedingExpression */
static int tokens_preceding_expression(Str s) {
  if (s.n == 0) return 1;
  if (str_is(s, "/") || str_is(s, "+") || str_is(s, "-") || str_is(s, "...")) return 1;
  if (str_is(s, "?InterpolationInJSX") || str_is(s, "?InterpolationInTemplate") ||
      str_is(s, "?NoLineTerminatorHere") || str_is(s, "?NonExpressionParenEnd") ||
      str_is(s, "?UnaryIncDec"))
    return 1;
  char c = s.p[s.n - 1];
  return c != '\0' && strchr("{}([,;<>=*%&|^!~?:", c) != NULL;
}

/* KeywordsWithExpressionAfter */
static int keywords_with_expression_after(Str s) {
  static const char *const kw[] = {"await", "case", "default", "delete", "do", "else", "instanceof",
                                   "new", "return", "throw", "typeof", "void", "yield"};
  for (size_t i = 0; i < sizeof kw / sizeof kw[0]; i++)
    if (str_is(s, kw[i])) return 1;
  return 0;
}

/* KeywordsWithNoLineTerminatorAfter */
static int keywords_no_line_terminator_after(Str s) {
  return str_is(s, "return") || str_is(s, "throw") || str_is(s, "yield");
}

/* TokensNotPrecedingObjectLiteral */
static int tokens_not_preceding_object_literal(Str s) {
  if (s.n == 0) return 1;
  if (str_is(s, "=>") || str_is(s, "else") || str_is(s, "?NoLineTerminatorHere") ||
      str_is(s, "?NonExpressionParenEnd"))
    return 1;
  return s.n == 1 && strchr(";]){}", s.p[0]) != NULL && s.p[0] != '\0';
}

/* ---- sticky matchers: each returns 1 and sets *end when the regex matches at i ---- */

/* Continues an X(?:_?X)* run starting at j, where ok() is the class X. */
static size_t tail_run(const uc *s, size_t n, size_t j, int (*ok)(uc)) {
  for (;;) {
    if (j < n && ok(s[j])) {
      j++;
    } else if (j + 1 < n && s[j] == '_' && ok(s[j + 1])) {
      j += 2;
    } else {
      return j;
    }
  }
}

/* /\/(?![*\/])(?:\[(?:(?![\]\\]).|\\.)*\]|(?![\/\\]).|\\.)*(\/[$_‌‍\p{ID_Continue}]*|\\)?/ */
static int match_regex(const uc *s, size_t n, size_t i, size_t *end) {
  size_t j = i + 1;
  if (j < n && (s[j] == '*' || s[j] == '/')) return 0;
  for (;;) {
    if (j >= n) break;
    uc c = s[j];
    if (c == '[') {
      /* class alternative: scan to the closing ']' on this line, else treat '[' as a plain char */
      size_t k = j + 1;
      int closed = 0;
      while (k < n) {
        if (s[k] == ']') {
          closed = 1;
          break;
        }
        if (s[k] == '\\') {
          if (k + 1 >= n || is_lt(s[k + 1])) break;
          k += 2;
        } else if (is_lt(s[k])) {
          break;
        } else {
          k++;
        }
      }
      if (closed) {
        j = k + 1;
        continue;
      }
      j++;
      continue;
    }
    if (c == '/') break;
    if (c == '\\') {
      if (j + 1 < n && !is_lt(s[j + 1])) {
        j += 2;
        continue;
      }
      break;
    }
    if (is_lt(c)) break;
    j++;
  }
  if (j < n && s[j] == '/') {
    j++;
    while (j < n && (is_ident_part(s[j]))) j++;
  } else if (j < n && s[j] == '\\') {
    j++;
  }
  *end = j;
  return 1;
}

/* Punctuator: --|\+\+|=>|\.{3}|(\?)?\.(?!\d)|(?:&&|\|\||\?\?|[+\-%&|^]|\*{1,2}|<{1,2}|>{1,3}|!=?|={1,2}|\/(?![\/*]))=?|[?~,:;[\](){}] */
static int match_punct(const uc *s, size_t n, size_t i, size_t *end) {
  uc c = s[i];
  uc c1 = i + 1 < n ? s[i + 1] : 0;
  uc c2 = i + 2 < n ? s[i + 2] : 0;
  if (c == '-' && c1 == '-') { *end = i + 2; return 1; }
  if (c == '+' && c1 == '+') { *end = i + 2; return 1; }
  if (c == '=' && c1 == '>') { *end = i + 2; return 1; }
  if (c == '.' && c1 == '.' && c2 == '.') { *end = i + 3; return 1; }
  if (c == '?' && c1 == '.' && !(i + 2 < n && is_digit(c2))) { *end = i + 2; return 1; }
  if (c == '.' && !(i + 1 < n && is_digit(c1))) { *end = i + 1; return 1; }
  size_t e = 0;
  int matched = 1;
  if (c == '&' && c1 == '&') e = i + 2;
  else if (c == '|' && c1 == '|') e = i + 2;
  else if (c == '?' && c1 == '?') e = i + 2;
  else if (c == '+' || c == '-' || c == '%' || c == '&' || c == '|' || c == '^') e = i + 1;
  else if (c == '*') e = (c1 == '*') ? i + 2 : i + 1;
  else if (c == '<') e = (c1 == '<') ? i + 2 : i + 1;
  else if (c == '>') {
    e = i + 1;
    if (c1 == '>') {
      e = i + 2;
      if (c2 == '>') e = i + 3;
    }
  } else if (c == '!') e = (c1 == '=') ? i + 2 : i + 1;
  else if (c == '=') e = (c1 == '=') ? i + 2 : i + 1;
  else if (c == '/' && c1 != '/' && c1 != '*') e = i + 1;
  else matched = 0;
  if (matched) {
    if (e < n && s[e] == '=') e++;
    *end = e;
    return 1;
  }
  if (c == '?' || c == '~' || c == ',' || c == ':' || c == ';' || c == '[' || c == ']' || c == '(' ||
      c == ')' || c == '{' || c == '}') {
    *end = i + 1;
    return 1;
  }
  return 0;
}

/* Identifier: (\x23?)(?=[$_\p{ID_Start}\\])(?:[$_ID_Continue]|\\u[hex]{4}|\\u\{[hex]+\})+ */
static int match_identifier(const uc *s, size_t n, size_t i, size_t *end) {
  size_t j = i;
  if (j < n && s[j] == '#') j++;
  if (j >= n || !(is_ident_start(s[j]) || s[j] == '\\')) return 0;
  size_t k = j;
  int count = 0;
  for (;;) {
    if (k < n && is_ident_part(s[k])) {
      k++;
      count++;
      continue;
    }
    if (k + 1 < n && s[k] == '\\' && s[k + 1] == 'u') {
      if (k + 5 < n && is_hex(s[k + 2]) && is_hex(s[k + 3]) && is_hex(s[k + 4]) && is_hex(s[k + 5])) {
        k += 6;
        count++;
        continue;
      }
      if (k + 2 < n && s[k + 2] == '{') {
        size_t m = k + 3;
        while (m < n && is_hex(s[m])) m++;
        if (m > k + 3 && m < n && s[m] == '}') {
          k = m + 1;
          count++;
          continue;
        }
      }
    }
    break;
  }
  if (count == 0) return 0;
  *end = k;
  return 1;
}

/* StringLiteral: (['"])(?:(?!\1)[^\\\n\r]|\\(?:\r\n|[^]))*(\1)? */
static int match_string(const uc *s, size_t n, size_t i, size_t *end) {
  uc q = s[i];
  if (q != '\'' && q != '"') return 0;
  size_t j = i + 1;
  while (j < n) {
    uc c = s[j];
    if (c == q || is_lt(c)) break;
    if (c == '\\') {
      if (j + 1 >= n) break;
      if (s[j + 1] == '\r' && j + 2 < n && s[j + 2] == '\n') j += 3;
      else j += 2;
      continue;
    }
    j++;
  }
  if (j < n && s[j] == q) j++;
  *end = j;
  return 1;
}

/* NumericLiteral, with its alternatives in the library's order. */
static int match_numeric(const uc *s, size_t n, size_t i, size_t *end) {
  uc c = s[i];
  uc c1 = i + 1 < n ? s[i + 1] : 0;
  uc c2 = i + 2 < n ? s[i + 2] : 0;
  size_t e;
  /* 0[xX][hex](?:_?[hex])*  |  0[oO][0-7]...  |  0[bB][01]...   then n? */
  if (c == '0' && (c1 == 'x' || c1 == 'X') && i + 2 < n && is_hex(c2)) {
    e = tail_run(s, n, i + 3, is_hex);
  } else if (c == '0' && (c1 == 'o' || c1 == 'O') && i + 2 < n && is_oct(c2)) {
    e = tail_run(s, n, i + 3, is_oct);
  } else if (c == '0' && (c1 == 'b' || c1 == 'B') && i + 2 < n && is_bin(c2)) {
    e = tail_run(s, n, i + 3, is_bin);
  } else {
    e = 0;
  }
  if (e) {
    if (e < n && s[e] == 'n') e++;
    *end = e;
    return 1;
  }
  if (c == '0' && c1 == 'n') { *end = i + 2; return 1; }
  if (c >= '1' && c <= '9') {
    size_t k = tail_run(s, n, i + 1, is_digit);
    if (k < n && s[k] == 'n') { *end = k + 1; return 1; }
  }
  /* (?:(?:0(?!\d)|0\d*[89]\d*|[1-9](?:_?\d)*)(?:\.(?:\d(?:_?\d)*)?)?|\.\d(?:_?\d)*)(?:[eE][+-]?\d(?:_?\d)*)? */
  int ok = 0;
  e = i;
  if (c == '0' && !(i + 1 < n && is_digit(c1))) {
    e = i + 1;
    ok = 1;
  } else if (c == '0') {
    size_t k = i + 1;
    int has89 = 0;
    while (k < n && is_digit(s[k])) {
      if (s[k] == '8' || s[k] == '9') has89 = 1;
      k++;
    }
    if (has89) {
      e = k;
      ok = 1;
    }
  } else if (c >= '1' && c <= '9') {
    e = tail_run(s, n, i + 1, is_digit);
    ok = 1;
  } else if (c == '.' && i + 1 < n && is_digit(c1)) {
    e = tail_run(s, n, i + 2, is_digit);
    ok = 1;
  }
  if (ok) {
    if (c != '.' && e < n && s[e] == '.') {
      e++;
      if (e < n && is_digit(s[e])) e = tail_run(s, n, e + 1, is_digit);
    }
    if (e < n && (s[e] == 'e' || s[e] == 'E')) {
      size_t k = e + 1;
      if (k < n && (s[k] == '+' || s[k] == '-')) k++;
      if (k < n && is_digit(s[k])) e = tail_run(s, n, k + 1, is_digit);
    }
    *end = e;
    return 1;
  }
  /* 0[0-7]+ */
  if (c == '0' && c1 >= '0' && c1 <= '7') {
    e = i + 1;
    while (e < n && is_oct(s[e])) e++;
    *end = e;
    return 1;
  }
  return 0;
}

/* Template: [`}](?:[^`\\$]|\\[^]|\$(?!\{))*(`|\$\{)?   kind: 1 closed by backtick, 2 "${", 0 open */
static int match_template(const uc *s, size_t n, size_t i, size_t *end, int *kind) {
  if (s[i] != '`' && s[i] != '}') return 0;
  size_t j = i + 1;
  while (j < n) {
    uc c = s[j];
    if (c == '`') break;
    if (c == '\\') {
      if (j + 1 >= n) break;
      j += 2;
      continue;
    }
    if (c == '$' && j + 1 < n && s[j + 1] == '{') break;
    j++;
  }
  *kind = 0;
  if (j < n && s[j] == '`') {
    j++;
    *kind = 1;
  } else if (j + 1 < n && s[j] == '$' && s[j + 1] == '{') {
    j += 2;
    *kind = 2;
  }
  *end = j;
  return 1;
}

/* WhiteSpace: [\t\v\f﻿\p{Zs}]+  (ASCII: tab, vt, ff, space) */
static int match_whitespace(const uc *s, size_t n, size_t i, size_t *end) {
  size_t j = i;
  while (j < n && (s[j] == '\t' || s[j] == '\v' || s[j] == '\f' || s[j] == ' ')) j++;
  if (j == i) return 0;
  *end = j;
  return 1;
}

/* LineTerminatorSequence: \r?\n|[\r  ] */
static int match_line_terminator(const uc *s, size_t n, size_t i, size_t *end) {
  if (s[i] == '\r' && i + 1 < n && s[i + 1] == '\n') { *end = i + 2; return 1; }
  if (s[i] == '\n' || s[i] == '\r') { *end = i + 1; return 1; }
  return 0;
}

/* MultiLineComment: \/\*(?:[^*]|\*(?!\/))*(\*\/)?   *has_newline set when the comment spans lines */
static int match_multiline_comment(const uc *s, size_t n, size_t i, size_t *end, int *has_newline) {
  if (!(s[i] == '/' && i + 1 < n && s[i + 1] == '*')) return 0;
  size_t j = i + 2;
  *has_newline = 0;
  while (j < n) {
    if (s[j] == '*' && j + 1 < n && s[j + 1] == '/') break;
    if (is_lt(s[j])) *has_newline = 1;
    j++;
  }
  if (j + 1 < n && s[j] == '*' && s[j + 1] == '/') j += 2;
  *end = j;
  return 1;
}

/* SingleLineComment: \/\/.* */
static int match_single_line_comment(const uc *s, size_t n, size_t i, size_t *end) {
  if (!(s[i] == '/' && i + 1 < n && s[i + 1] == '/')) return 0;
  size_t j = i + 2;
  while (j < n && !is_lt(s[j])) j++;
  *end = j;
  return 1;
}

/* JSXPunctuator: [<>.:={}]|\/(?![\/*]) */
static int match_jsx_punct(const uc *s, size_t n, size_t i, size_t *end) {
  uc c = s[i];
  if (c == '<' || c == '>' || c == '.' || c == ':' || c == '=' || c == '{' || c == '}') {
    *end = i + 1;
    return 1;
  }
  if (c == '/' && !(i + 1 < n && (s[i + 1] == '/' || s[i + 1] == '*'))) {
    *end = i + 1;
    return 1;
  }
  return 0;
}

/* JSXIdentifier: [$_\p{ID_Start}][$_‌‍\p{ID_Continue}-]* */
static int match_jsx_identifier(const uc *s, size_t n, size_t i, size_t *end) {
  if (!is_ident_start(s[i])) return 0;
  size_t j = i + 1;
  while (j < n && (is_ident_part(s[j]) || s[j] == '-')) j++;
  *end = j;
  return 1;
}

/* JSXString: (['"])(?:(?!\1)[^])*(\1)? */
static int match_jsx_string(const uc *s, size_t n, size_t i, size_t *end) {
  uc q = s[i];
  if (q != '\'' && q != '"') return 0;
  size_t j = i + 1;
  while (j < n && s[j] != q) j++;
  if (j < n) j++;
  *end = j;
  return 1;
}

/* JSXText: [^<>{}]+ */
static int match_jsx_text(const uc *s, size_t n, size_t i, size_t *end) {
  size_t j = i;
  while (j < n && !(s[j] == '<' || s[j] == '>' || s[j] == '{' || s[j] == '}')) j++;
  if (j == i) return 0;
  *end = j;
  return 1;
}

/* ---- the tokenizer: one call yields (counts) every token of the input ---- */
enum { M_JS, M_JS_NON_EXPRESSION_PAREN, M_INTERPOLATION_IN_TEMPLATE, M_INTERPOLATION_IN_JSX,
       M_JSX_TAG, M_JSX_TAG_END, M_JSX_CHILDREN };

typedef struct {
  int tag;
  long nesting;
} Mode;

#define MAX_DEPTH 4096

static long tokenize(const uc *s, size_t length, int jsx) {
  static Mode modes[MAX_DEPTH];
  static int braces[MAX_DEPTH];
  int sp = 0, bsp = 0;
  long parenNesting = 0;
  int postfixIncDec = 0;
  size_t lastIndex = 0;
  Str lastSig = lit("");
  long count = 0;

  modes[sp].tag = M_JS;
  modes[sp].nesting = 0;
  sp++;

  while (lastIndex < length) {
    Mode mode = modes[sp - 1];
    switch (mode.tag) {
    case M_JS:
    case M_JS_NON_EXPRESSION_PAREN:
    case M_INTERPOLATION_IN_TEMPLATE:
    case M_INTERPOLATION_IN_JSX: {
      size_t e;
      if (s[lastIndex] == '/' && (tokens_preceding_expression(lastSig) || keywords_with_expression_after(lastSig))) {
        if (match_regex(s, length, lastIndex, &e)) {
          lastSig = span(s, lastIndex, e);
          lastIndex = e;
          postfixIncDec = 1;
          count++;
          continue;
        }
      }
      if (match_punct(s, length, lastIndex, &e)) {
        Str punct = span(s, lastIndex, e);
        Str nextSig = punct;
        if (str_is(punct, "(")) {
          if (str_is(lastSig, "?NonExpressionParenKeyword")) {
            modes[sp].tag = M_JS_NON_EXPRESSION_PAREN;
            modes[sp].nesting = parenNesting;
            sp++;
          }
          parenNesting++;
          postfixIncDec = 0;
        } else if (str_is(punct, ")")) {
          parenNesting--;
          postfixIncDec = 1;
          if (mode.tag == M_JS_NON_EXPRESSION_PAREN && parenNesting == mode.nesting) {
            sp--;
            nextSig = lit("?NonExpressionParenEnd");
            postfixIncDec = 0;
          }
        } else if (str_is(punct, "{")) {
          int isExpression = !tokens_not_preceding_object_literal(lastSig) &&
                             (tokens_preceding_expression(lastSig) || keywords_with_expression_after(lastSig));
          braces[bsp++] = isExpression;
          postfixIncDec = 0;
        } else if (str_is(punct, "}")) {
          int handled = 0;
          if (mode.tag == M_INTERPOLATION_IN_TEMPLATE && bsp == mode.nesting) {
            size_t te;
            int kind;
            match_template(s, length, lastIndex, &te, &kind);
            lastSig = span(s, lastIndex, te);
            lastIndex = te;
            if (kind == 2) {
              lastSig = lit("?InterpolationInTemplate");
              postfixIncDec = 0;
            } else {
              sp--;
              postfixIncDec = 1;
            }
            count++;
            continue;
          }
          if (mode.tag == M_INTERPOLATION_IN_JSX && bsp == mode.nesting) {
            sp--;
            lastIndex += 1;
            lastSig = lit("}");
            count++;
            continue;
          }
          if (!handled) {
            postfixIncDec = bsp > 0 ? braces[--bsp] : 0;
            nextSig = postfixIncDec ? lit("?ExpressionBraceEnd") : lit("}");
          }
        } else if (str_is(punct, "]")) {
          postfixIncDec = 1;
        } else if (str_is(punct, "++") || str_is(punct, "--")) {
          nextSig = postfixIncDec ? lit("?PostfixIncDec") : lit("?UnaryIncDec");
        } else if (str_is(punct, "<")) {
          if (jsx && (tokens_preceding_expression(lastSig) || keywords_with_expression_after(lastSig))) {
            modes[sp].tag = M_JSX_TAG;
            modes[sp].nesting = 0;
            sp++;
            lastIndex += 1;
            lastSig = lit("<");
            count++;
            continue;
          }
          postfixIncDec = 0;
        } else {
          postfixIncDec = 0;
        }
        lastIndex = e;
        lastSig = nextSig;
        count++;
        continue;
      }
      if (match_identifier(s, length, lastIndex, &e)) {
        Str text = span(s, lastIndex, e);
        Str nextSig = text;
        lastIndex = e;
        if (str_is(text, "for") || str_is(text, "if") || str_is(text, "while") || str_is(text, "with")) {
          if (!str_is(lastSig, ".") && !str_is(lastSig, "?.")) nextSig = lit("?NonExpressionParenKeyword");
        }
        lastSig = nextSig;
        postfixIncDec = !keywords_with_expression_after(text);
        count++;
        continue;
      }
      if (match_string(s, length, lastIndex, &e)) {
        lastSig = span(s, lastIndex, e);
        lastIndex = e;
        postfixIncDec = 1;
        count++;
        continue;
      }
      if (match_numeric(s, length, lastIndex, &e)) {
        lastSig = span(s, lastIndex, e);
        lastIndex = e;
        postfixIncDec = 1;
        count++;
        continue;
      }
      {
        int kind;
        if (match_template(s, length, lastIndex, &e, &kind)) {
          lastSig = span(s, lastIndex, e);
          lastIndex = e;
          if (kind == 2) {
            lastSig = lit("?InterpolationInTemplate");
            modes[sp].tag = M_INTERPOLATION_IN_TEMPLATE;
            modes[sp].nesting = bsp;
            sp++;
            postfixIncDec = 0;
          } else {
            postfixIncDec = 1;
          }
          count++;
          continue;
        }
      }
      break;
    }
    case M_JSX_TAG:
    case M_JSX_TAG_END: {
      size_t e;
      if (match_jsx_punct(s, length, lastIndex, &e)) {
        Str text = span(s, lastIndex, e);
        Str nextSig = text;
        lastIndex = e;
        if (str_is(text, "<")) {
          modes[sp].tag = M_JSX_TAG;
          modes[sp].nesting = 0;
          sp++;
        } else if (str_is(text, ">")) {
          sp--;
          if (str_is(lastSig, "/") || mode.tag == M_JSX_TAG_END) {
            nextSig = lit("?JSX");
            postfixIncDec = 1;
          } else {
            modes[sp].tag = M_JSX_CHILDREN;
            modes[sp].nesting = 0;
            sp++;
          }
        } else if (str_is(text, "{")) {
          modes[sp].tag = M_INTERPOLATION_IN_JSX;
          modes[sp].nesting = bsp;
          sp++;
          nextSig = lit("?InterpolationInJSX");
          postfixIncDec = 0;
        } else if (str_is(text, "/")) {
          if (str_is(lastSig, "<")) {
            sp--;
            if (modes[sp - 1].tag == M_JSX_CHILDREN) sp--;
            modes[sp].tag = M_JSX_TAG_END;
            modes[sp].nesting = 0;
            sp++;
          }
        }
        lastSig = nextSig;
        count++;
        continue;
      }
      if (match_jsx_identifier(s, length, lastIndex, &e)) {
        lastSig = span(s, lastIndex, e);
        lastIndex = e;
        count++;
        continue;
      }
      if (match_jsx_string(s, length, lastIndex, &e)) {
        lastSig = span(s, lastIndex, e);
        lastIndex = e;
        count++;
        continue;
      }
      break;
    }
    case M_JSX_CHILDREN: {
      size_t e;
      if (match_jsx_text(s, length, lastIndex, &e)) {
        lastSig = span(s, lastIndex, e);
        lastIndex = e;
        count++;
        continue;
      }
      if (s[lastIndex] == '<') {
        modes[sp].tag = M_JSX_TAG;
        modes[sp].nesting = 0;
        sp++;
        lastIndex++;
        lastSig = lit("<");
        count++;
        continue;
      }
      if (s[lastIndex] == '{') {
        modes[sp].tag = M_INTERPOLATION_IN_JSX;
        modes[sp].nesting = bsp;
        sp++;
        lastIndex++;
        lastSig = lit("?InterpolationInJSX");
        postfixIncDec = 0;
        count++;
        continue;
      }
      break;
    }
    }

    /* Shared tail: whitespace, line terminators, comments, then one invalid character. */
    {
      size_t e;
      int hasNewline;
      if (match_whitespace(s, length, lastIndex, &e)) {
        lastIndex = e;
        count++;
        continue;
      }
      if (match_line_terminator(s, length, lastIndex, &e)) {
        lastIndex = e;
        postfixIncDec = 0;
        if (keywords_no_line_terminator_after(lastSig)) lastSig = lit("?NoLineTerminatorHere");
        count++;
        continue;
      }
      if (match_multiline_comment(s, length, lastIndex, &e, &hasNewline)) {
        lastIndex = e;
        if (hasNewline) {
          postfixIncDec = 0;
          if (keywords_no_line_terminator_after(lastSig)) lastSig = lit("?NoLineTerminatorHere");
        }
        count++;
        continue;
      }
      if (match_single_line_comment(s, length, lastIndex, &e)) {
        lastIndex = e;
        postfixIncDec = 0;
        count++;
        continue;
      }
      lastSig = span(s, lastIndex, lastIndex + 1);
      lastIndex += 1;
      postfixIncDec = 0;
      count++;
    }
  }
  return count;
}

/* ---- the benchmark inputs (JS source text of the tokenizer, and the JSX sample) ---- */
static const char JS_SOURCE[] =
  "function jsTokensWrapperFunction() {\n"
  "    var Identifier, JSXIdentifier, JSXPunctuator, JSXString, JSXText, KeywordsWithExpressionAfter, KeywordsWithNoLineTerminatorAfter, LineTerminatorSequence, MultiLineComment, Newline, NumericLiteral, Punctuator, RegularExpressionLiteral, SingleLineComment, StringLiteral, Template, TokensNotPrecedingObjectLiteral, TokensPrecedingExpression, WhiteSpace, jsTokens;\n"
  "    RegularExpressionLiteral = /\\/(\?![*\\/])(\?:\\[(\?:(\?![\\]\\\\]).|\\\\.)*\\]|(\?![\\/\\\\]).|\\\\.)*(\\/[$_\\u200C\\u200D\\p{ID_Continue}]*|\\\\)\?/yu;\n"
  "    Punctuator = /--|\\+\\+|=>|\\.{3}|\\\?\?\\.(\?!\\d)|(\?:&&|\\|\\||\\\?\\\?|[+\\-%&|^]|\\*{1,2}|<{1,2}|>{1,3}|!=\?|={1,2}|\\/(\?![\\/*]))=\?|[\?~,:;[\\](){}]/y;\n"
  "    Identifier = /(\\x23\?)(\?=[$_\\p{ID_Start}\\\\])(\?:[$_\\u200C\\u200D\\p{ID_Continue}]|\\\\u[\\da-fA-F]{4}|\\\\u\\{[\\da-fA-F]+\\})+/yu;\n"
  "    StringLiteral = /(['\"])(\?:(\?!\\1)[^\\\\\\n\\r]|\\\\(\?:\\r\\n|[^]))*(\\1)\?/y;\n"
  "    NumericLiteral = /(\?:0[xX][\\da-fA-F](\?:_\?[\\da-fA-F])*|0[oO][0-7](\?:_\?[0-7])*|0[bB][01](\?:_\?[01])*)n\?|0n|[1-9](\?:_\?\\d)*n|(\?:(\?:0(\?!\\d)|0\\d*[89]\\d*|[1-9](\?:_\?\\d)*)(\?:\\.(\?:\\d(\?:_\?\\d)*)\?)\?|\\.\\d(\?:_\?\\d)*)(\?:[eE][+-]\?\\d(\?:_\?\\d)*)\?|0[0-7]+/y;\n"
  "    Template = /[`}](\?:[^`\\\\$]|\\\\[^]|\\$(\?!\\{))*(`|\\$\\{)\?/y;\n"
  "    WhiteSpace = /[\\t\\v\\f\\ufeff\\p{Zs}]+/yu;\n"
  "    LineTerminatorSequence = /\\r\?\\n|[\\r\\u2028\\u2029]/y;\n"
  "    MultiLineComment = /\\/\\*(\?:[^*]|\\*(\?!\\/))*(\\*\\/)\?/y;\n"
  "    SingleLineComment = /\\/\\/.*/y;\n"
  "    JSXPunctuator = /[<>.:={}]|\\/(\?![\\/*])/y;\n"
  "    JSXIdentifier = /[$_\\p{ID_Start}][$_\\u200C\\u200D\\p{ID_Continue}-]*/yu;\n"
  "    JSXString = /(['\"])(\?:(\?!\\1)[^])*(\\1)\?/y;\n"
  "    JSXText = /[^<>{}]+/y;\n"
  "    TokensPrecedingExpression = /^(\?:[\\/+-]|\\.{3}|\\\?(\?:InterpolationIn(\?:JSX|Template)|NoLineTerminatorHere|NonExpressionParenEnd|UnaryIncDec))\?$|[{}([,;<>=*%&|^!~\?:]$/;\n"
  "    TokensNotPrecedingObjectLiteral = /^(\?:=>|[;\\]){}]|else|\\\?(\?:NoLineTerminatorHere|NonExpressionParenEnd))\?$/;\n"
  "    KeywordsWithExpressionAfter = /^(\?:await|case|default|delete|do|else|instanceof|new|return|throw|typeof|void|yield)$/;\n"
  "    KeywordsWithNoLineTerminatorAfter = /^(\?:return|throw|yield)$/;\n"
  "    Newline = RegExp(LineTerminatorSequence.source);\n"
  "    return function*(input, {jsx = false} = {}) {\n"
  "        var braces, firstCodePoint, isExpression, lastIndex, lastSignificantToken, length, match, mode, nextLastIndex, nextLastSignificantToken, parenNesting, postfixIncDec, punctuator, stack;\n"
  "        ({length} = input);\n"
  "        lastIndex = 0;\n"
  "        lastSignificantToken = \"\";\n"
  "        stack = [\n"
  "            {tag: \"JS\"}\n"
  "        ];\n"
  "        braces = [];\n"
  "        parenNesting = 0;\n"
  "        postfixIncDec = false;\n"
  "        while (lastIndex < length) {\n"
  "            mode = stack[stack.length - 1];\n"
  "            switch (mode.tag) {\n"
  "                case \"JS\":\n"
  "                case \"JSNonExpressionParen\":\n"
  "                case \"InterpolationInTemplate\":\n"
  "                case \"InterpolationInJSX\":\n"
  "                    if (input[lastIndex] === \"/\" && (TokensPrecedingExpression.test(lastSignificantToken) || KeywordsWithExpressionAfter.test(lastSignificantToken))) {\n"
  "                        RegularExpressionLiteral.lastIndex = lastIndex;\n"
  "                        if (match = RegularExpressionLiteral.exec(input)) {\n"
  "                            lastIndex = RegularExpressionLiteral.lastIndex;\n"
  "                            lastSignificantToken = match[0];\n"
  "                            postfixIncDec = true;\n"
  "                            yield ({\n"
  "                                type: \"RegularExpressionLiteral\",\n"
  "                                value: match[0],\n"
  "                                closed: match[1] !== void 0 && match[1] !== \"\\\\\"\n"
  "                            });\n"
  "                            continue;\n"
  "                        }\n"
  "                    }\n"
  "                    Punctuator.lastIndex = lastIndex;\n"
  "                    if (match = Punctuator.exec(input)) {\n"
  "                        punctuator = match[0];\n"
  "                        nextLastIndex = Punctuator.lastIndex;\n"
  "                        nextLastSignificantToken = punctuator;\n"
  "                        switch (punctuator) {\n"
  "                            case \"(\":\n"
  "                                if (lastSignificantToken === \"\?NonExpressionParenKeyword\") {\n"
  "                                    stack.push({\n"
  "                                        tag: \"JSNonExpressionParen\",\n"
  "                                        nesting: parenNesting\n"
  "                                    });\n"
  "                                }\n"
  "                                parenNesting++;\n"
  "                                postfixIncDec = false;\n"
  "                                break;\n"
  "                            case \")\":\n"
  "                                parenNesting--;\n"
  "                                postfixIncDec = true;\n"
  "                                if (mode.tag === \"JSNonExpressionParen\" && parenNesting === mode.nesting) {\n"
  "                                    stack.pop();\n"
  "                                    nextLastSignificantToken = \"\?NonExpressionParenEnd\";\n"
  "                                    postfixIncDec = false;\n"
  "                                }\n"
  "                                break;\n"
  "                            case \"{\":\n"
  "                                Punctuator.lastIndex = 0;\n"
  "                                isExpression = !TokensNotPrecedingObjectLiteral.test(lastSignificantToken) && (TokensPrecedingExpression.test(lastSignificantToken) || KeywordsWithExpressionAfter.test(lastSignificantToken));\n"
  "                                braces.push(isExpression);\n"
  "                                postfixIncDec = false;\n"
  "                                break;\n"
  "                            case \"}\":\n"
  "                                switch (mode.tag) {\n"
  "                                    case \"InterpolationInTemplate\":\n"
  "                                        if (braces.length === mode.nesting) {\n"
  "                                            Template.lastIndex = lastIndex;\n"
  "                                            match = Template.exec(input);\n"
  "                                            lastIndex = Template.lastIndex;\n"
  "                                            lastSignificantToken = match[0];\n"
  "                                            if (match[1] === \"${\") {\n"
  "                                                lastSignificantToken = \"\?InterpolationInTemplate\";\n"
  "                                                postfixIncDec = false;\n"
  "                                                yield ({\n"
  "                                                    type: \"TemplateMiddle\",\n"
  "                                                    value: match[0]\n"
  "                                                });\n"
  "                                            } else {\n"
  "                                                stack.pop();\n"
  "                                                postfixIncDec = true;\n"
  "                                                yield ({\n"
  "                                                    type: \"TemplateTail\",\n"
  "                                                    value: match[0],\n"
  "                                                    closed: match[1] === \"`\"\n"
  "                                                });\n"
  "                                            }\n"
  "                                            continue;\n"
  "                                        }\n"
  "                                        break;\n"
  "                                    case \"InterpolationInJSX\":\n"
  "                                        if (braces.length === mode.nesting) {\n"
  "                                            stack.pop();\n"
  "                                            lastIndex += 1;\n"
  "                                            lastSignificantToken = \"}\";\n"
  "                                            yield ({\n"
  "                                                type: \"JSXPunctuator\",\n"
  "                                                value: \"}\"\n"
  "                                            });\n"
  "                                            continue;\n"
  "                                        }\n"
  "                                }\n"
  "                                postfixIncDec = braces.pop();\n"
  "                                nextLastSignificantToken = postfixIncDec \? \"\?ExpressionBraceEnd\" : \"}\";\n"
  "                                break;\n"
  "                            case \"]\":\n"
  "                                postfixIncDec = true;\n"
  "                                break;\n"
  "                            case \"++\":\n"
  "                            case \"--\":\n"
  "                                nextLastSignificantToken = postfixIncDec \? \"\?PostfixIncDec\" : \"\?UnaryIncDec\";\n"
  "                                break;\n"
  "                            case \"<\":\n"
  "                                if (jsx && (TokensPrecedingExpression.test(lastSignificantToken) || KeywordsWithExpressionAfter.test(lastSignificantToken))) {\n"
  "                                    stack.push({tag: \"JSXTag\"});\n"
  "                                    lastIndex += 1;\n"
  "                                    lastSignificantToken = \"<\";\n"
  "                                    yield ({\n"
  "                                        type: \"JSXPunctuator\",\n"
  "                                        value: punctuator\n"
  "                                    });\n"
  "                                    continue;\n"
  "                                }\n"
  "                                postfixIncDec = false;\n"
  "                                break;\n"
  "                            default:\n"
  "                                postfixIncDec = false;\n"
  "                        }\n"
  "                        lastIndex = nextLastIndex;\n"
  "                        lastSignificantToken = nextLastSignificantToken;\n"
  "                        yield ({\n"
  "                            type: \"Punctuator\",\n"
  "                            value: punctuator\n"
  "                        });\n"
  "                        continue;\n"
  "                    }\n"
  "                    Identifier.lastIndex = lastIndex;\n"
  "                    if (match = Identifier.exec(input)) {\n"
  "                        lastIndex = Identifier.lastIndex;\n"
  "                        nextLastSignificantToken = match[0];\n"
  "                        switch (match[0]) {\n"
  "                            case \"for\":\n"
  "                            case \"if\":\n"
  "                            case \"while\":\n"
  "                            case \"with\":\n"
  "                                if (lastSignificantToken !== \".\" && lastSignificantToken !== \"\?.\") {\n"
  "                                    nextLastSignificantToken = \"\?NonExpressionParenKeyword\";\n"
  "                                }\n"
  "                        }\n"
  "                        lastSignificantToken = nextLastSignificantToken;\n"
  "                        postfixIncDec = !KeywordsWithExpressionAfter.test(match[0]);\n"
  "                        yield ({\n"
  "                            type: match[1] === \"#\" \? \"PrivateIdentifier\" : \"IdentifierName\",\n"
  "                            value: match[0]\n"
  "                        });\n"
  "                        continue;\n"
  "                    }\n"
  "                    StringLiteral.lastIndex = lastIndex;\n"
  "                    if (match = StringLiteral.exec(input)) {\n"
  "                        lastIndex = StringLiteral.lastIndex;\n"
  "                        lastSignificantToken = match[0];\n"
  "                        postfixIncDec = true;\n"
  "                        yield ({\n"
  "                            type: \"StringLiteral\",\n"
  "                            value: match[0],\n"
  "                            closed: match[2] !== void 0\n"
  "                        });\n"
  "                        continue;\n"
  "                    }\n"
  "                    NumericLiteral.lastIndex = lastIndex;\n"
  "                    if (match = NumericLiteral.exec(input)) {\n"
  "                        lastIndex = NumericLiteral.lastIndex;\n"
  "                        lastSignificantToken = match[0];\n"
  "                        postfixIncDec = true;\n"
  "                        yield ({\n"
  "                            type: \"NumericLiteral\",\n"
  "                            value: match[0]\n"
  "                        });\n"
  "                        continue;\n"
  "                    }\n"
  "                    Template.lastIndex = lastIndex;\n"
  "                    if (match = Template.exec(input)) {\n"
  "                        lastIndex = Template.lastIndex;\n"
  "                        lastSignificantToken = match[0];\n"
  "                        if (match[1] === \"${\") {\n"
  "                            lastSignificantToken = \"\?InterpolationInTemplate\";\n"
  "                            stack.push({\n"
  "                                tag: \"InterpolationInTemplate\",\n"
  "                                nesting: braces.length\n"
  "                            });\n"
  "                            postfixIncDec = false;\n"
  "                            yield ({\n"
  "                                type: \"TemplateHead\",\n"
  "                                value: match[0]\n"
  "                            });\n"
  "                        } else {\n"
  "                            postfixIncDec = true;\n"
  "                            yield ({\n"
  "                                type: \"NoSubstitutionTemplate\",\n"
  "                                value: match[0],\n"
  "                                closed: match[1] === \"`\"\n"
  "                            });\n"
  "                        }\n"
  "                        continue;\n"
  "                    }\n"
  "                    break;\n"
  "                case \"JSXTag\":\n"
  "                case \"JSXTagEnd\":\n"
  "                    JSXPunctuator.lastIndex = lastIndex;\n"
  "                    if (match = JSXPunctuator.exec(input)) {\n"
  "                        lastIndex = JSXPunctuator.lastIndex;\n"
  "                        nextLastSignificantToken = match[0];\n"
  "                        switch (match[0]) {\n"
  "                            case \"<\":\n"
  "                                stack.push({tag: \"JSXTag\"});\n"
  "                                break;\n"
  "                            case \">\":\n"
  "                                stack.pop();\n"
  "                                if (lastSignificantToken === \"/\" || mode.tag === \"JSXTagEnd\") {\n"
  "                                    nextLastSignificantToken = \"\?JSX\";\n"
  "                                    postfixIncDec = true;\n"
  "                                } else {\n"
  "                                    stack.push({tag: \"JSXChildren\"});\n"
  "                                }\n"
  "                                break;\n"
  "                            case \"{\":\n"
  "                                stack.push({\n"
  "                                    tag: \"InterpolationInJSX\",\n"
  "                                    nesting: braces.length\n"
  "                                });\n"
  "                                nextLastSignificantToken = \"\?InterpolationInJSX\";\n"
  "                                postfixIncDec = false;\n"
  "                                break;\n"
  "                            case \"/\":\n"
  "                                if (lastSignificantToken === \"<\") {\n"
  "                                    stack.pop();\n"
  "                                    if (stack[stack.length - 1].tag === \"JSXChildren\") {\n"
  "                                        stack.pop();\n"
  "                                    }\n"
  "                                    stack.push({tag: \"JSXTagEnd\"});\n"
  "                                }\n"
  "                        }\n"
  "                        lastSignificantToken = nextLastSignificantToken;\n"
  "                        yield ({\n"
  "                            type: \"JSXPunctuator\",\n"
  "                            value: match[0]\n"
  "                        });\n"
  "                        continue;\n"
  "                    }\n"
  "                    JSXIdentifier.lastIndex = lastIndex;\n"
  "                    if (match = JSXIdentifier.exec(input)) {\n"
  "                        lastIndex = JSXIdentifier.lastIndex;\n"
  "                        lastSignificantToken = match[0];\n"
  "                        yield ({\n"
  "                            type: \"JSXIdentifier\",\n"
  "                            value: match[0]\n"
  "                        });\n"
  "                        continue;\n"
  "                    }\n"
  "                    JSXString.lastIndex = lastIndex;\n"
  "                    if (match = JSXString.exec(input)) {\n"
  "                        lastIndex = JSXString.lastIndex;\n"
  "                        lastSignificantToken = match[0];\n"
  "                        yield ({\n"
  "                            type: \"JSXString\",\n"
  "                            value: match[0],\n"
  "                            closed: match[2] !== void 0\n"
  "                        });\n"
  "                        continue;\n"
  "                    }\n"
  "                    break;\n"
  "                case \"JSXChildren\":\n"
  "                    JSXText.lastIndex = lastIndex;\n"
  "                    if (match = JSXText.exec(input)) {\n"
  "                        lastIndex = JSXText.lastIndex;\n"
  "                        lastSignificantToken = match[0];\n"
  "                        yield ({\n"
  "                            type: \"JSXText\",\n"
  "                            value: match[0]\n"
  "                        });\n"
  "                        continue;\n"
  "                    }\n"
  "                    switch (input[lastIndex]) {\n"
  "                        case \"<\":\n"
  "                            stack.push({tag: \"JSXTag\"});\n"
  "                            lastIndex++;\n"
  "                            lastSignificantToken = \"<\";\n"
  "                            yield ({\n"
  "                                type: \"JSXPunctuator\",\n"
  "                                value: \"<\"\n"
  "                            });\n"
  "                            continue;\n"
  "                        case \"{\":\n"
  "                            stack.push({\n"
  "                                tag: \"InterpolationInJSX\",\n"
  "                                nesting: braces.length\n"
  "                            });\n"
  "                            lastIndex++;\n"
  "                            lastSignificantToken = \"\?InterpolationInJSX\";\n"
  "                            postfixIncDec = false;\n"
  "                            yield ({\n"
  "                                type: \"JSXPunctuator\",\n"
  "                                value: \"{\"\n"
  "                            });\n"
  "                            continue;\n"
  "                    }\n"
  "            }\n"
  "            WhiteSpace.lastIndex = lastIndex;\n"
  "            if (match = WhiteSpace.exec(input)) {\n"
  "                lastIndex = WhiteSpace.lastIndex;\n"
  "                yield ({\n"
  "                    type: \"WhiteSpace\",\n"
  "                    value: match[0]\n"
  "                });\n"
  "                continue;\n"
  "            }\n"
  "            LineTerminatorSequence.lastIndex = lastIndex;\n"
  "            if (match = LineTerminatorSequence.exec(input)) {\n"
  "                lastIndex = LineTerminatorSequence.lastIndex;\n"
  "                postfixIncDec = false;\n"
  "                if (KeywordsWithNoLineTerminatorAfter.test(lastSignificantToken)) {\n"
  "                    lastSignificantToken = \"\?NoLineTerminatorHere\";\n"
  "                }\n"
  "                yield ({\n"
  "                    type: \"LineTerminatorSequence\",\n"
  "                    value: match[0]\n"
  "                });\n"
  "                continue;\n"
  "            }\n"
  "            MultiLineComment.lastIndex = lastIndex;\n"
  "            if (match = MultiLineComment.exec(input)) {\n"
  "                lastIndex = MultiLineComment.lastIndex;\n"
  "                if (Newline.test(match[0])) {\n"
  "                    postfixIncDec = false;\n"
  "                    if (KeywordsWithNoLineTerminatorAfter.test(lastSignificantToken)) {\n"
  "                        lastSignificantToken = \"\?NoLineTerminatorHere\";\n"
  "                    }\n"
  "                }\n"
  "                yield ({\n"
  "                    type: \"MultiLineComment\",\n"
  "                    value: match[0],\n"
  "                    closed: match[1] !== void 0\n"
  "                });\n"
  "                continue;\n"
  "            }\n"
  "            SingleLineComment.lastIndex = lastIndex;\n"
  "            if (match = SingleLineComment.exec(input)) {\n"
  "                lastIndex = SingleLineComment.lastIndex;\n"
  "                postfixIncDec = false;\n"
  "                yield ({\n"
  "                    type: \"SingleLineComment\",\n"
  "                    value: match[0]\n"
  "                });\n"
  "                continue;\n"
  "            }\n"
  "            firstCodePoint = String.fromCodePoint(input.codePointAt(lastIndex));\n"
  "            lastIndex += firstCodePoint.length;\n"
  "            lastSignificantToken = firstCodePoint;\n"
  "            postfixIncDec = false;\n"
  "            yield ({\n"
  "                type: mode.tag.startsWith(\"JSX\") \? \"JSXInvalid\" : \"Invalid\",\n"
  "                value: firstCodePoint\n"
  "            });\n"
  "        }\n"
  "    };\n"
  "}"
  ;
static const char JSX_SOURCE[] =
  "\n"
  "\n"
  "function Comment(props) {\n"
  "  return (\n"
  "    <div className=\"Comment\">\n"
  "      <div className=\"UserInfo\">\n"
  "        <Avatar user={props.author} />\n"
  "        <div className=\"UserInfo-name\">\n"
  "          {props.author.name}\n"
  "        </div>\n"
  "      </div>\n"
  "      <div className=\"Comment-text\">\n"
  "        {props.text}\n"
  "      </div>\n"
  "      <div className=\"Comment-date\">\n"
  "        {formatDate(props.date)}\n"
  "      </div>\n"
  "    </div>\n"
  "  );\n"
  "}\n"
  "\n"
  ;

static long run_iteration(void) {
  long count = 0;
  for (int i = 0; i < 25; i++) count += tokenize((const uc *)JS_SOURCE, sizeof JS_SOURCE - 1, 0);
  for (int i = 0; i < 250; i++) count += tokenize((const uc *)JSX_SOURCE, sizeof JSX_SOURCE - 1, 1);
  return count;
}

int main(int argc, char **argv) {
  int iterations = argc > 1 ? atoi(argv[1]) : 120;
  struct timespec a, b;
  long tokenCount = 0;
  clock_gettime(CLOCK_MONOTONIC, &a);
  for (int i = 0; i < iterations; i++) tokenCount = run_iteration();
  clock_gettime(CLOCK_MONOTONIC, &b);
  printf("js-tokens %d iterations %.3f ms\n", iterations, (b.tv_sec - a.tv_sec) * 1e3 + (b.tv_nsec - a.tv_nsec) / 1e6);
  printf("tokenCount %ld\n", tokenCount);
  if (tokenCount != 113975) {
    fprintf(stderr, "this.tokenCount of %ld is invalid!\n", tokenCount);
    return 1;
  }
  return 0;
}
