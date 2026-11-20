/*
 * TypeScript 5.9.2 scanner, ported from createScanner() in lib/typescript.js (lines 12114-13330).
 * One global scanner state (`TS`), as the JS closure has one scanner per parse.
 *
 * Differences that do not change tokens, node counts or parse decisions:
 *  - string / template / regex / numeric values are kept as raw source slices (no escape decoding,
 *    no numeric value computation). Identifier values are raw slices too (escapes are consumed,
 *    not decoded).
 *  - scanner errors only increment TS.errors (the parser counts them for speculation rollback).
 *  - non-ASCII bytes are not identifier characters (none occur in identifiers in the immer-tiny set).
 */
#ifndef TS_SCANNER_H
#define TS_SCANNER_H

#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include "ts-enums.h"

/* TokenFlags (typescript.js TokenFlags enum). */
enum {
  TF_PrecedingLineBreak = 1,
  TF_PrecedingJSDocComment = 2,
  TF_Unterminated = 4,
  TF_ExtendedUnicodeEscape = 8,
  TF_Scientific = 16,
  TF_Octal = 32,
  TF_HexSpecifier = 64,
  TF_BinarySpecifier = 128,
  TF_OctalSpecifier = 256,
  TF_ContainsSeparator = 512,
  TF_UnicodeEscape = 1024,
  TF_ContainsInvalidEscape = 2048,
  TF_HexEscape = 4096,
  TF_ContainsLeadingZero = 8192,
  TF_ContainsInvalidSeparator = 16384,
  TF_PrecedingJSDocLeadingAsterisks = 32768,
};

/* Sentinel for "no character" (typescript.js EOF = -1). */
#define TS_EOF (-1)

typedef struct TsScanner {
  const char *text;   /* source bytes (not NUL terminated semantics: use len) */
  int len;
  int pos, end;
  int fullStart, tokenStart;
  int token;
  const char *tval;   /* token value slice (identifiers, literals) */
  int tvalLen;
  uint32_t tflags;
  int languageVariant;   /* 0 standard, 1 JSX */
  int scriptKind;
  int jsDocParsingMode;  /* 0 ParseAll, 1 ParseNone, 2 ParseForTypeErrors, 3 ParseForTypeInfo */
  int skipJsDocLeadingAsterisks;
  long errors;           /* parse/scan error count (messages are not needed for the counts) */
} TsScanner;

static TsScanner TS;

/* ---- character classes (typescript.js isIdentifierStart / isIdentifierPart for ASCII) ---- */
static inline bool ts_isDigit(int c) { return c >= '0' && c <= '9'; }
static inline bool ts_isOctalDigit(int c) { return c >= '0' && c <= '7'; }
static inline bool ts_isHexDigit(int c) { return ts_isDigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
static inline bool ts_isIdStart(int c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '$' || c == '_'; }
static inline bool ts_isIdPart(int c) { return ts_isIdStart(c) || ts_isDigit(c); }
static inline bool ts_isLineBreak(int c) { return c == '\n' || c == '\r' || c == 0x2028 || c == 0x2029; }
static inline bool ts_isWhiteSpaceSingleLine(int c) {
  return c == ' ' || c == '\t' || c == 0x0b || c == 0x0c || c == 0xa0 || c == 0xfeff;
}

static inline int ts_charAt(int pos) {
  return (pos >= 0 && pos < TS.len) ? (unsigned char)TS.text[pos] : 0;
}
/* charCodeUnchecked: reads even past end (0 when out of the buffer). */
static inline int ts_cu(int pos) { return ts_charAt(pos); }
static inline int ts_cc(int pos) { return (pos >= 0 && pos < TS.end) ? ts_charAt(pos) : TS_EOF; }

static void ts_scan_error(void) { TS.errors++; }

/* Keyword lookup: TS keyword kinds indexed by identifier text (typescript.js textToKeywordObj). */
typedef struct { const char *name; int kind; } TsKeywordEntry;
static const TsKeywordEntry ts_keywords[] = {
  {"abstract",128},{"accessor",129},{"any",133},{"as",130},{"asserts",131},{"assert",132},{"bigint",163},
  {"boolean",136},{"break",83},{"case",84},{"catch",85},{"class",86},{"continue",88},{"const",87},
  {"constructor",137},{"debugger",89},{"declare",138},{"default",90},{"defer",166},{"delete",91},{"do",92},{"else",93},
  {"enum",94},{"export",95},{"extends",96},{"false",97},{"finally",98},{"for",99},{"from",161},
  {"function",100},{"get",139},{"if",101},{"implements",119},{"import",102},{"in",103},{"infer",140},
  {"instanceof",104},{"interface",120},{"intrinsic",141},{"is",142},{"keyof",143},{"let",121},
  {"module",144},{"namespace",145},{"never",146},{"new",105},{"null",106},{"number",150},{"object",151},
  {"package",122},{"private",123},{"protected",124},{"public",125},{"override",164},{"out",147},
  {"readonly",148},{"require",149},{"global",162},{"return",107},{"satisfies",152},{"set",153},
  {"static",126},{"string",154},{"super",108},{"switch",109},{"symbol",155},{"this",110},{"throw",111},
  {"true",112},{"try",113},{"type",156},{"typeof",114},{"undefined",157},{"unique",158},{"unknown",159},
  {"using",160},{"var",115},{"void",116},{"while",117},{"with",118},{"yield",127},{"async",134},
  {"await",135},{"of",165},
};
#define TS_NKEYWORDS ((int)(sizeof(ts_keywords) / sizeof(ts_keywords[0])))

/* getIdentifierToken: keyword kind for the current token value, else Identifier. */
static int ts_keywordKind(const char *s, int n) {
  if (n < 2 || n > 12 || s[0] < 'a' || s[0] > 'z') return -1;
  for (int i = 0; i < TS_NKEYWORDS; i++) {
    const char *k = ts_keywords[i].name;
    if ((int)strlen(k) == n && memcmp(k, s, n) == 0) return ts_keywords[i].kind;
  }
  return -1;
}

static int ts_getIdentifierToken(void) {
  int kw = ts_keywordKind(TS.tval, TS.tvalLen);
  TS.token = kw >= 0 ? kw : TSK_Identifier;
  return TS.token;
}

/* ---- setup / accessors ---- */
static void ts_setText(const char *text, int len) {
  TS.text = text;
  TS.len = len;
  TS.pos = 0;
  TS.end = len;
  TS.fullStart = 0;
  TS.tokenStart = 0;
  TS.token = TSK_Unknown;
  TS.tval = "";
  TS.tvalLen = 0;
  TS.tflags = 0;
}

static void ts_resetTokenState(int pos) {
  TS.pos = pos;
  TS.fullStart = pos;
  TS.tokenStart = pos;
  TS.token = TSK_Unknown;
  TS.tflags = 0;
}

/* Scans a number fragment (digits with separators). Returns nothing; pos is advanced. */
static void ts_scanDigits(void) {
  while (ts_isDigit(ts_cc(TS.pos))) TS.pos++;
}

static void ts_scanNumberFragment(void) {
  bool allowSeparator = false, prevSep = false;
  for (;;) {
    int ch = ts_cu(TS.pos);
    if (ch == '_') {
      TS.tflags |= TF_ContainsSeparator;
      if (allowSeparator) {
        allowSeparator = false;
        prevSep = true;
      } else {
        TS.tflags |= TF_ContainsInvalidSeparator;
        ts_scan_error();
      }
      TS.pos++;
      continue;
    }
    if (ts_isDigit(ch)) {
      allowSeparator = true;
      prevSep = false;
      TS.pos++;
      continue;
    }
    break;
  }
  (void)prevSep;
  if (ts_cu(TS.pos - 1) == '_') {
    TS.tflags |= TF_ContainsInvalidSeparator;
    ts_scan_error();
  }
}

/* scanHexDigits / binary / octal: consume digits of a radix (separators allowed). */
static int ts_scanRadixDigits(int base, bool hex, int minCount, bool canHaveSeparators) {
  int count = 0;
  bool allowSeparator = false;
  for (;;) {
    int ch = ts_cu(TS.pos);
    if (canHaveSeparators && ch == '_') {
      TS.tflags |= TF_ContainsSeparator;
      if (allowSeparator) allowSeparator = false;
      else ts_scan_error();
      TS.pos++;
      continue;
    }
    allowSeparator = canHaveSeparators;
    bool ok = hex ? ts_isHexDigit(ch) : (ts_isDigit(ch) && ch - '0' < base);
    if (!ok) break;
    count++;
    TS.pos++;
  }
  if (count < minCount) ts_scan_error();
  if (ts_cu(TS.pos - 1) == '_') ts_scan_error();
  return count;
}

/* checkForIdentifierStartAfterNumericLiteral */
static void ts_checkIdentifierAfterNumber(int numericStart) {
  if (!ts_isIdStart(ts_cu(TS.pos))) return;
  int identifierStart = TS.pos;
  while (TS.pos < TS.end && ts_isIdPart(ts_cu(TS.pos))) TS.pos++;
  int length = TS.pos - identifierStart;
  if (length == 1 && ts_cu(identifierStart) == 'n') {
    (void)numericStart;  /* bigint suffix: handled by checkBigIntSuffix in the caller */
  } else {
    ts_scan_error();
    TS.pos = identifierStart;
  }
}

/* scanNumber: returns NumericLiteral or BigIntLiteral. */
static int ts_scanNumber(void) {
  int start = TS.pos;
  /* 0x / 0b / 0o prefixes (typescript.js scan(), case 48) */
  if (ts_cu(TS.pos) == '0' && TS.pos + 2 < TS.end) {
    int c1 = ts_cu(TS.pos + 1);
    int radix = (c1 == 'x' || c1 == 'X') ? 16 : (c1 == 'b' || c1 == 'B') ? 2 : (c1 == 'o' || c1 == 'O') ? 8 : 0;
    if (radix) {
      TS.pos += 2;
      int digits = ts_scanRadixDigits(radix, radix == 16, 1, true);
      if (!digits) ts_scan_error();
      TS.tflags |= radix == 16 ? TF_HexSpecifier : radix == 2 ? TF_BinarySpecifier : TF_OctalSpecifier;
      TS.tval = TS.text + start;
      TS.tvalLen = TS.pos - start;
      if (ts_cu(TS.pos) == 'n') {
        TS.pos++;
        TS.tvalLen = TS.pos - start;
        return TSK_BigIntLiteral;
      }
      ts_checkIdentifierAfterNumber(start);
      return TSK_NumericLiteral;
    }
  }
  if (ts_cu(TS.pos) == '0') {
    TS.pos++;
    if (ts_cu(TS.pos) == '_') {
      TS.tflags |= TF_ContainsSeparator | TF_ContainsInvalidSeparator;
      ts_scan_error();
      TS.pos--;
      ts_scanNumberFragment();
    } else {
      /* scanDigits: leading-zero forms */
      int s = TS.pos;
      bool isOctal = true;
      while (ts_isDigit(ts_cc(TS.pos))) {
        if (!ts_isOctalDigit(ts_cu(TS.pos))) isOctal = false;
        TS.pos++;
      }
      if (!isOctal) {
        TS.tflags |= TF_ContainsLeadingZero;
      } else if (TS.pos == s) {
        /* plain 0: no digits, fall to fraction/exponent handling */
      } else {
        TS.tflags |= TF_Octal;
        ts_scan_error();
        TS.tval = TS.text + start;
        TS.tvalLen = TS.pos - start;
        return TSK_NumericLiteral;
      }
    }
  } else {
    ts_scanNumberFragment();
  }
  int numEnd = TS.pos;
  if (ts_cu(TS.pos) == '.') {
    TS.pos++;
    ts_scanNumberFragment();
    numEnd = TS.pos;
  }
  if (ts_cu(TS.pos) == 'e' || ts_cu(TS.pos) == 'E') {
    TS.pos++;
    TS.tflags |= TF_Scientific;
    if (ts_cu(TS.pos) == '+' || ts_cu(TS.pos) == '-') TS.pos++;
    int before = TS.pos;
    ts_scanNumberFragment();
    if (TS.pos == before) ts_scan_error();
    else numEnd = TS.pos;
  }
  TS.tval = TS.text + start;
  TS.tvalLen = numEnd - start;
  if (TS.tflags & TF_ContainsLeadingZero) {
    ts_scan_error();
    return TSK_NumericLiteral;
  }
  if (numEnd != start + (TS.pos - start) || (TS.tflags & (TF_Scientific))) {
    /* fraction or exponent: no bigint suffix */
  }
  bool hasFractionOrExp = (numEnd > start) && (TS.tflags & TF_Scientific || memchr(TS.text + start, '.', numEnd - start));
  if (hasFractionOrExp) {
    TS.pos = numEnd;
    ts_checkIdentifierAfterNumber(start);
    return TSK_NumericLiteral;
  }
  /* bigint suffix */
  if (ts_cu(TS.pos) == 'n') {
    TS.pos++;
    TS.tvalLen = TS.pos - start;
    return TSK_BigIntLiteral;
  }
  ts_checkIdentifierAfterNumber(start);
  return TSK_NumericLiteral;
}

/* scanString: raw slice without the quotes is not needed, the token value keeps the raw text. */
static void ts_scanString(bool jsxAttribute) {
  int quote = ts_cu(TS.pos);
  TS.pos++;
  int start = TS.pos;
  for (;;) {
    if (TS.pos >= TS.end) {
      TS.tflags |= TF_Unterminated;
      ts_scan_error();
      break;
    }
    int ch = ts_cu(TS.pos);
    if (ch == quote) {
      TS.pos++;
      break;
    }
    if (ch == '\\' && !jsxAttribute) {
      /* escape: consume the escape introducer and the next character (multi-char escapes are
         consumed by the following loop iterations since they contain no quote or backslash) */
      TS.pos++;
      if (TS.pos < TS.end) {
        int e = ts_cu(TS.pos);
        if (e == '\r' && ts_cu(TS.pos + 1) == '\n') TS.pos++;
        TS.pos++;
      }
      continue;
    }
    if ((ch == '\n' || ch == '\r') && !jsxAttribute) {
      TS.tflags |= TF_Unterminated;
      ts_scan_error();
      break;
    }
    TS.pos++;
  }
  TS.tval = TS.text + start;
  TS.tvalLen = TS.pos - start - ((TS.tflags & TF_Unterminated) ? 0 : 1);
}

/* scanTemplateAndSetTokenValue: returns TemplateHead/Middle/Tail/NoSubstitutionTemplateLiteral. */
static int ts_scanTemplate(bool startedWithBacktick_unused) {
  (void)startedWithBacktick_unused;
  bool startedWithBacktick = ts_cu(TS.pos) == '`';
  TS.pos++;
  int start = TS.pos;
  int result;
  for (;;) {
    if (TS.pos >= TS.end) {
      TS.tflags |= TF_Unterminated;
      ts_scan_error();
      result = startedWithBacktick ? TSK_NoSubstitutionTemplateLiteral : TSK_TemplateTail;
      break;
    }
    int c = ts_cu(TS.pos);
    if (c == '`') {
      TS.pos++;
      result = startedWithBacktick ? TSK_NoSubstitutionTemplateLiteral : TSK_TemplateTail;
      break;
    }
    if (c == '$' && TS.pos + 1 < TS.end && ts_cu(TS.pos + 1) == '{') {
      TS.pos += 2;
      result = startedWithBacktick ? TSK_TemplateHead : TSK_TemplateMiddle;
      break;
    }
    if (c == '\\') {
      TS.pos += 2;
      continue;
    }
    TS.pos++;
  }
  TS.tval = TS.text + start;
  TS.tvalLen = TS.pos - start;
  return result;
}

/* scanEscapeSequence is consumed inside ts_scanString/template. Identifier escapes below. */
static int ts_scanUnicodeEscapeInIdent(void) {
  /* at '\\' followed by 'u': returns consumed length in chars (4 hex or {hex}), 0 if invalid */
  if (ts_cu(TS.pos + 1) != 'u') return 0;
  if (ts_cu(TS.pos + 2) == '{') {
    int p = TS.pos + 3, n = 0;
    while (ts_isHexDigit(ts_cu(p))) { p++; n++; }
    if (n == 0 || ts_cu(p) != '}') return 0;
    TS.pos = p + 1;
    TS.tflags |= TF_ExtendedUnicodeEscape;
    return 1;
  }
  for (int i = 0; i < 4; i++) {
    if (!ts_isHexDigit(ts_cu(TS.pos + 2 + i))) return 0;
  }
  TS.pos += 6;
  TS.tflags |= TF_UnicodeEscape;
  return 1;
}

/* scanIdentifierParts for escape-containing identifiers. */
static void ts_scanIdentifierParts(void) {
  while (TS.pos < TS.end) {
    int ch = ts_cu(TS.pos);
    if (ts_isIdPart(ch)) {
      TS.pos++;
    } else if (ch == '\\') {
      if (!ts_scanUnicodeEscapeInIdent()) break;
    } else {
      break;
    }
  }
}

/* scanIdentifier: start is an identifier start at TS.pos. Returns identifier token kind or 0. */
static int ts_scanIdentifier(void) {
  int startChar = ts_cu(TS.pos);
  if (!ts_isIdStart(startChar)) return 0;
  TS.pos++;
  while (TS.pos < TS.end && ts_isIdPart(ts_cu(TS.pos))) TS.pos++;
  TS.tval = TS.text + TS.tokenStart;
  TS.tvalLen = TS.pos - TS.tokenStart;
  if (ts_cu(TS.pos) == '\\') {
    ts_scanIdentifierParts();
    TS.tvalLen = TS.pos - TS.tokenStart;
  }
  return ts_getIdentifierToken();
}

/* Trivia and comments. Returns true if the caller should `continue` scanning (trivia skipped). */
static bool ts_shouldParseJSDoc(void) {
  switch (TS.jsDocParsingMode) {
    case 0: return true;   /* ParseAll */
    case 1: return false;  /* ParseNone */
  }
  if (TS.scriptKind != 3 && TS.scriptKind != 4) return true;
  if (TS.jsDocParsingMode == 3) return false;
  return true; /* ParseForTypeErrors on JS: approximated, not used for .ts/.d.ts */
}

/* scan(): the next token with trivia skipped (skipTrivia = true, as the parser uses). */
static int ts_scan(void) {
  TS.fullStart = TS.pos;
  TS.tflags = 0;
  for (;;) {
    TS.tokenStart = TS.pos;
    if (TS.pos >= TS.end) return TS.token = TSK_EndOfFileToken;
    int ch = ts_cu(TS.pos);
    if (TS.pos == 0 && ch == '#' && ts_cu(1) == '!') {
      /* shebang: skipped as trivia */
      while (TS.pos < TS.end && !ts_isLineBreak(ts_cu(TS.pos))) TS.pos++;
      continue;
    }
    switch (ch) {
      case '\n':
      case '\r':
        TS.tflags |= TF_PrecedingLineBreak;
        TS.pos++;
        continue;
      case '\t': case 0x0b: case 0x0c: case ' ':
        TS.pos++;
        continue;
      case '!':
        if (ts_cu(TS.pos + 1) == '=') {
          if (ts_cu(TS.pos + 2) == '=') return TS.pos += 3, TS.token = TSK_ExclamationEqualsEqualsToken;
          return TS.pos += 2, TS.token = TSK_ExclamationEqualsToken;
        }
        TS.pos++;
        return TS.token = TSK_ExclamationToken;
      case '"':
      case '\'':
        ts_scanString(false);
        return TS.token = TSK_StringLiteral;
      case '`':
        return TS.token = ts_scanTemplate(false);
      case '%':
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_PercentEqualsToken;
        TS.pos++;
        return TS.token = TSK_PercentToken;
      case '&':
        if (ts_cu(TS.pos + 1) == '&') {
          if (ts_cu(TS.pos + 2) == '=') return TS.pos += 3, TS.token = TSK_AmpersandAmpersandEqualsToken;
          return TS.pos += 2, TS.token = TSK_AmpersandAmpersandToken;
        }
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_AmpersandEqualsToken;
        TS.pos++;
        return TS.token = TSK_AmpersandToken;
      case '(':
        TS.pos++;
        return TS.token = TSK_OpenParenToken;
      case ')':
        TS.pos++;
        return TS.token = TSK_CloseParenToken;
      case '*':
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_AsteriskEqualsToken;
        if (ts_cu(TS.pos + 1) == '*') {
          if (ts_cu(TS.pos + 2) == '=') return TS.pos += 3, TS.token = TSK_AsteriskAsteriskEqualsToken;
          return TS.pos += 2, TS.token = TSK_AsteriskAsteriskToken;
        }
        TS.pos++;
        if (TS.skipJsDocLeadingAsterisks && !(TS.tflags & TF_PrecedingJSDocLeadingAsterisks) && (TS.tflags & TF_PrecedingLineBreak)) {
          TS.tflags |= TF_PrecedingJSDocLeadingAsterisks;
          continue;
        }
        return TS.token = TSK_AsteriskToken;
      case '+':
        if (ts_cu(TS.pos + 1) == '+') return TS.pos += 2, TS.token = TSK_PlusPlusToken;
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_PlusEqualsToken;
        TS.pos++;
        return TS.token = TSK_PlusToken;
      case ',':
        TS.pos++;
        return TS.token = TSK_CommaToken;
      case '-':
        if (ts_cu(TS.pos + 1) == '-') return TS.pos += 2, TS.token = TSK_MinusMinusToken;
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_MinusEqualsToken;
        TS.pos++;
        return TS.token = TSK_MinusToken;
      case '.':
        if (ts_isDigit(ts_cu(TS.pos + 1))) {
          ts_scanNumber();
          return TS.token = TSK_NumericLiteral;
        }
        if (ts_cu(TS.pos + 1) == '.' && ts_cu(TS.pos + 2) == '.') return TS.pos += 3, TS.token = TSK_DotDotDotToken;
        TS.pos++;
        return TS.token = TSK_DotToken;
      case '/':
        if (ts_cu(TS.pos + 1) == '/') {
          TS.pos += 2;
          while (TS.pos < TS.end && !ts_isLineBreak(ts_cu(TS.pos))) TS.pos++;
          continue;
        }
        if (ts_cu(TS.pos + 1) == '*') {
          TS.pos += 2;
          bool isJSDoc = ts_cu(TS.pos) == '*' && ts_cu(TS.pos + 1) != '/';
          bool closed = false;
          while (TS.pos < TS.end) {
            int c2 = ts_cu(TS.pos);
            if (c2 == '*' && ts_cu(TS.pos + 1) == '/') {
              TS.pos += 2;
              closed = true;
              break;
            }
            TS.pos++;
            if (ts_isLineBreak(c2)) TS.tflags |= TF_PrecedingLineBreak;
          }
          if (isJSDoc && ts_shouldParseJSDoc()) TS.tflags |= TF_PrecedingJSDocComment;
          if (!closed) {
            TS.tflags |= TF_Unterminated;
            ts_scan_error();
          }
          continue;
        }
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_SlashEqualsToken;
        TS.pos++;
        return TS.token = TSK_SlashToken;
      case '0': case '1': case '2': case '3': case '4':
      case '5': case '6': case '7': case '8': case '9':
        return TS.token = ts_scanNumber();
      case ':':
        TS.pos++;
        return TS.token = TSK_ColonToken;
      case ';':
        TS.pos++;
        return TS.token = TSK_SemicolonToken;
      case '<':
        if (ts_cu(TS.pos + 1) == '<') {
          if (ts_cu(TS.pos + 2) == '=') return TS.pos += 3, TS.token = TSK_LessThanLessThanEqualsToken;
          return TS.pos += 2, TS.token = TSK_LessThanLessThanToken;
        }
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_LessThanEqualsToken;
        if (TS.languageVariant == 1 && ts_cu(TS.pos + 1) == '/' && ts_cu(TS.pos + 2) != '*') return TS.pos += 2, TS.token = TSK_LessThanSlashToken;
        TS.pos++;
        return TS.token = TSK_LessThanToken;
      case '=':
        if (ts_cu(TS.pos + 1) == '=') {
          if (ts_cu(TS.pos + 2) == '=') return TS.pos += 3, TS.token = TSK_EqualsEqualsEqualsToken;
          return TS.pos += 2, TS.token = TSK_EqualsEqualsToken;
        }
        if (ts_cu(TS.pos + 1) == '>') return TS.pos += 2, TS.token = TSK_EqualsGreaterThanToken;
        TS.pos++;
        return TS.token = TSK_EqualsToken;
      case '>':
        TS.pos++;
        return TS.token = TSK_GreaterThanToken;
      case '?':
        if (ts_cu(TS.pos + 1) == '.' && !ts_isDigit(ts_cu(TS.pos + 2))) return TS.pos += 2, TS.token = TSK_QuestionDotToken;
        if (ts_cu(TS.pos + 1) == '?') {
          if (ts_cu(TS.pos + 2) == '=') return TS.pos += 3, TS.token = TSK_QuestionQuestionEqualsToken;
          return TS.pos += 2, TS.token = TSK_QuestionQuestionToken;
        }
        TS.pos++;
        return TS.token = TSK_QuestionToken;
      case '[':
        TS.pos++;
        return TS.token = TSK_OpenBracketToken;
      case ']':
        TS.pos++;
        return TS.token = TSK_CloseBracketToken;
      case '^':
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_CaretEqualsToken;
        TS.pos++;
        return TS.token = TSK_CaretToken;
      case '{':
        TS.pos++;
        return TS.token = TSK_OpenBraceToken;
      case '|':
        if (ts_cu(TS.pos + 1) == '|') {
          if (ts_cu(TS.pos + 2) == '=') return TS.pos += 3, TS.token = TSK_BarBarEqualsToken;
          return TS.pos += 2, TS.token = TSK_BarBarToken;
        }
        if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_BarEqualsToken;
        TS.pos++;
        return TS.token = TSK_BarToken;
      case '}':
        TS.pos++;
        return TS.token = TSK_CloseBraceToken;
      case '~':
        TS.pos++;
        return TS.token = TSK_TildeToken;
      case '@':
        TS.pos++;
        return TS.token = TSK_AtToken;
      case '\\': {
        /* identifier starting with a unicode escape */
        int save = TS.pos;
        if (ts_scanUnicodeEscapeInIdent()) {
          TS.tflags |= 0;
          ts_scanIdentifierParts();
          TS.tval = TS.text + TS.tokenStart;
          TS.tvalLen = TS.pos - TS.tokenStart;
          return ts_getIdentifierToken();
        }
        (void)save;
        ts_scan_error();
        TS.pos++;
        return TS.token = TSK_Unknown;
      }
      case '#': {
        if (TS.pos != 0 && ts_cu(TS.pos + 1) == '!') {
          ts_scan_error();
          TS.pos++;
          return TS.token = TSK_Unknown;
        }
        int after = ts_cu(TS.pos + 1);
        if (after == '\\') {
          TS.pos++;
          if (ts_scanUnicodeEscapeInIdent()) {
            ts_scanIdentifierParts();
            TS.tval = TS.text + TS.tokenStart;
            TS.tvalLen = TS.pos - TS.tokenStart;
            return TS.token = TSK_PrivateIdentifier;
          }
          TS.pos--;
        }
        if (ts_isIdStart(after)) {
          TS.pos++;
          ts_scanIdentifier();
        } else {
          ts_scan_error();
          TS.pos++;
        }
        TS.tval = TS.text + TS.tokenStart;
        TS.tvalLen = TS.pos - TS.tokenStart;
        return TS.token = TSK_PrivateIdentifier;
      }
      default: {
        int kind = ts_scanIdentifier();
        if (kind) return TS.token = kind;
        if (ts_isWhiteSpaceSingleLine(ch)) {
          TS.pos++;
          continue;
        }
        if (ts_isLineBreak(ch)) {
          TS.tflags |= TF_PrecedingLineBreak;
          TS.pos++;
          continue;
        }
        ts_scan_error();
        TS.pos++;
        return TS.token = TSK_Unknown;
      }
    }
  }
}

/* reScanGreaterToken: '>' may be part of '>>', '>=', '>>=', '>>>', '>>>='. */
static int ts_reScanGreaterToken(void) {
  if (TS.token == TSK_GreaterThanToken) {
    if (ts_cu(TS.pos) == '>') {
      if (ts_cu(TS.pos + 1) == '>') {
        if (ts_cu(TS.pos + 2) == '=') return TS.pos += 3, TS.token = TSK_GreaterThanGreaterThanGreaterThanEqualsToken;
        return TS.pos += 2, TS.token = TSK_GreaterThanGreaterThanGreaterThanToken;
      }
      if (ts_cu(TS.pos + 1) == '=') return TS.pos += 2, TS.token = TSK_GreaterThanGreaterThanEqualsToken;
      TS.pos++;
      return TS.token = TSK_GreaterThanGreaterThanToken;
    }
    if (ts_cu(TS.pos) == '=') {
      TS.pos++;
      return TS.token = TSK_GreaterThanEqualsToken;
    }
  }
  return TS.token;
}

static int ts_reScanAsteriskEqualsToken(void) {
  TS.pos = TS.tokenStart + 1;
  return TS.token = TSK_AsteriskToken;
}

/* reScanSlashToken: a '/' or '/=' starts a regular expression literal. */
static int ts_reScanSlashToken(void) {
  if (TS.token == TSK_SlashToken || TS.token == TSK_SlashEqualsToken) {
    int p = TS.tokenStart + 1;
    TS.pos = p;
    bool inEscape = false, inClass = false;
    for (;;) {
      int ch = ts_cc(TS.pos);
      if (ch == TS_EOF || ts_isLineBreak(ch)) {
        TS.tflags |= TF_Unterminated;
        ts_scan_error();
        break;
      }
      if (inEscape) {
        inEscape = false;
      } else if (ch == '/' && !inClass) {
        TS.pos++;
        break;
      } else if (ch == '[') {
        inClass = true;
      } else if (ch == '\\') {
        inEscape = true;
      } else if (ch == ']') {
        inClass = false;
      }
      TS.pos++;
    }
    while (TS.pos < TS.end && ts_isIdPart(ts_cu(TS.pos))) TS.pos++;  /* flags */
    TS.tval = TS.text + TS.tokenStart;
    TS.tvalLen = TS.pos - TS.tokenStart;
    return TS.token = TSK_RegularExpressionLiteral;
  }
  return TS.token;
}

/* reScanTemplateToken: a '}' that closes a template substitution continues the template. */
static int ts_reScanTemplateToken(void) {
  TS.pos = TS.tokenStart;
  return TS.token = ts_scanTemplate(false);
}

static int ts_reScanLessThanToken(void) {
  if (TS.token == TSK_LessThanLessThanToken) {
    TS.pos = TS.tokenStart + 1;
    return TS.token = TSK_LessThanToken;
  }
  return TS.token;
}

static int ts_reScanHashToken(void) {
  if (TS.token == TSK_PrivateIdentifier) {
    TS.pos = TS.tokenStart + 1;
    return TS.token = TSK_Unknown;
  }
  return TS.token;
}

static int ts_reScanQuestionToken(void) {
  if (TS.token == TSK_QuestionQuestionToken) {
    TS.pos = TS.tokenStart + 1;
    return TS.token = TSK_QuestionToken;
  }
  return TS.token;
}

/* reScanInvalidIdentifier: re-lex an Unknown token as an identifier if possible. */
static int ts_reScanInvalidIdentifier(void) {
  TS.pos = TS.tokenStart = TS.fullStart;
  TS.tflags = 0;
  int k = ts_scanIdentifier();
  if (k) return TS.token = k;
  TS.pos++;
  return TS.token;
}

/* tryScan / lookAhead: run cb and roll back the token state unless it returns true (tryScan) */

/* scanner speculation state (speculationHelper in the JS scanner) */
typedef struct TsScanState { int pos, fullStart, tokenStart, token; const char *tval; int tvalLen; uint32_t tflags; } TsScanState;
static TsScanState ts_scanner_save(void) {
  TsScanState s = { TS.pos, TS.fullStart, TS.tokenStart, TS.token, TS.tval, TS.tvalLen, TS.tflags };
  return s;
}
static void ts_scanner_restore(const TsScanState *s) {
  TS.pos = s->pos;
  TS.fullStart = s->fullStart;
  TS.tokenStart = s->tokenStart;
  TS.token = s->token;
  TS.tval = s->tval;
  TS.tvalLen = s->tvalLen;
  TS.tflags = s->tflags;
}
/* tryScan(() => reScanInvalidIdentifier() === Identifier) */
static bool ts_tryScan_reScanInvalidIdentifier(void) {
  TsScanState s = ts_scanner_save();
  if (ts_reScanInvalidIdentifier() == TSK_Identifier) return true;
  ts_scanner_restore(&s);
  return false;
}


static inline bool ts_isWhiteSpaceLike(int c) { return ts_isWhiteSpaceSingleLine(c) || ts_isLineBreak(c); }

/* scanJsDocToken(): the JSDoc tokens (typescript.js scanJsDocToken). */
static int ts_scanJsDocToken(void) {
  TS.fullStart = TS.tokenStart = TS.pos;
  TS.tflags = 0;
  if (TS.pos >= TS.end) return TS.token = TSK_EndOfFileToken;
  int ch = ts_cu(TS.pos);
  TS.pos++;
  switch (ch) {
    case '\t': case '\v': case '\f': case ' ':
      while (TS.pos < TS.end && ts_isWhiteSpaceSingleLine(ts_cu(TS.pos))) TS.pos++;
      return TS.token = TSK_WhitespaceTrivia;
    case '@': return TS.token = TSK_AtToken;
    case '\r':
      if (ts_cu(TS.pos) == '\n') TS.pos++;
      /* fall through */
    case '\n':
      TS.tflags |= TF_PrecedingLineBreak;
      return TS.token = TSK_NewLineTrivia;
    case '*': return TS.token = TSK_AsteriskToken;
    case '{': return TS.token = TSK_OpenBraceToken;
    case '}': return TS.token = TSK_CloseBraceToken;
    case '[': return TS.token = TSK_OpenBracketToken;
    case ']': return TS.token = TSK_CloseBracketToken;
    case '(': return TS.token = TSK_OpenParenToken;
    case ')': return TS.token = TSK_CloseParenToken;
    case '<': return TS.token = TSK_LessThanToken;
    case '>': return TS.token = TSK_GreaterThanToken;
    case '=': return TS.token = TSK_EqualsToken;
    case ',': return TS.token = TSK_CommaToken;
    case '.': return TS.token = TSK_DotToken;
    case '`': return TS.token = TSK_BacktickToken;
    case '#': return TS.token = TSK_HashToken;
    case '\\': {
      /* a backslash starts an identifier only with a valid unicode escape (JS: pos--, peek, pos += 6) */
      TS.pos--;
      if (ts_cu(TS.pos) == '\\' && ts_scanUnicodeEscapeInIdent()) {
        ts_scanIdentifierParts();
        TS.tval = TS.text + TS.tokenStart;
        TS.tvalLen = TS.pos - TS.tokenStart;
        return TS.token = ts_getIdentifierToken();
      }
      TS.pos++;
      return TS.token = TSK_Unknown;
    }
  }
  if (ts_isIdStart(ch)) {
    int c = ch;
    while (TS.pos < TS.end && (ts_isIdPart(c = ts_cu(TS.pos)) || c == '-')) TS.pos++;
    TS.tval = TS.text + TS.tokenStart;
    TS.tvalLen = TS.pos - TS.tokenStart;
    if (c == '\\') {
      ts_scanIdentifierParts();
      TS.tvalLen = TS.pos - TS.tokenStart;
    }
    return TS.token = ts_getIdentifierToken();
  }
  return TS.token = TSK_Unknown;
}

/* scanJSDocCommentTextToken(inBackticks) */
static int ts_scanJSDocCommentTextToken(bool inBackticks) {
  TS.fullStart = TS.tokenStart = TS.pos;
  TS.tflags = 0;
  if (TS.pos >= TS.end) return TS.token = TSK_EndOfFileToken;
  for (int ch = ts_cu(TS.pos); TS.pos < TS.end && !ts_isLineBreak(ch) && ch != '`'; ch = ts_cu(++TS.pos)) {
    if (!inBackticks) {
      if (ch == '{') {
        break;
      } else if (ch == '@' && TS.pos - 1 >= 0 && ts_isWhiteSpaceSingleLine(ts_cu(TS.pos - 1)) &&
                 !(TS.pos + 1 < TS.end && ts_isWhiteSpaceLike(ts_cu(TS.pos + 1)))) {
        break;
      }
    }
  }
  if (TS.pos == TS.tokenStart) return ts_scanJsDocToken();
  TS.tval = TS.text + TS.tokenStart;
  TS.tvalLen = TS.pos - TS.tokenStart;
  return TS.token = TSK_JSDocCommentTextToken;
}

#endif
