/* js-parser-analogue.h: one hand-written JavaScript tokenizer and recursive-descent parser
 * shared by the C analogues of JetStream's JavaScript-parsing rows (Babylon, the code-load
 * rows). It produces an AST of heap-free node records carved from an arena, with node types
 * named after the Babel AST so the node and token counts can be compared with what Babel
 * produces for the same source (see babylon.c). Scope: ES2015-ES2020 syntax as used by the
 * row inputs: scripts and modules, functions, arrows, classes (methods, accessors, static),
 * destructuring, spread/rest, template literals, regular expressions, optional chaining,
 * nullish coalescing, async/await, generators, for-in/of/await, labels, switch, try,
 * import/export (named, default, namespace, star-from). Class fields, private names, JSX,
 * Flow, export-as-namespace and HTML-like comments are not supported: the parser stops with
 * a message and exits with status 3, so a gap in coverage is never silently skipped.
 *
 * Token counting follows Babel's `tokens` array with comments excluded and the EOF token
 * included. A template literal contributes a backquote token, one template token per chunk,
 * a "${" token per substitution and the closing backquote, as Babel does. */
#ifndef JS_PARSER_ANALOGUE_H
#define JS_PARSER_ANALOGUE_H

#include <ctype.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/* ---- arena: nodes and lists live until the next js_parse on the same context ---- */
typedef struct JsChunk { struct JsChunk *next; size_t cap, used; char data[]; } JsChunk;
typedef struct { JsChunk *head, *cur; } JsArena;

/* ---- AST ---- */
enum JsType {
  N_File, N_Program, N_Directive, N_DirectiveLiteral,
  N_ExpressionStatement, N_BlockStatement, N_EmptyStatement, N_DebuggerStatement, N_WithStatement,
  N_ReturnStatement, N_LabeledStatement, N_BreakStatement, N_ContinueStatement, N_IfStatement,
  N_SwitchStatement, N_SwitchCase, N_ThrowStatement, N_TryStatement, N_CatchClause,
  N_WhileStatement, N_DoWhileStatement, N_ForStatement, N_ForInStatement, N_ForOfStatement,
  N_FunctionDeclaration, N_VariableDeclaration, N_VariableDeclarator,
  N_ClassDeclaration, N_ClassExpression, N_ClassBody, N_ClassMethod,
  N_ImportDeclaration, N_ImportSpecifier, N_ImportDefaultSpecifier, N_ImportNamespaceSpecifier,
  N_ExportNamedDeclaration, N_ExportSpecifier, N_ExportDefaultDeclaration, N_ExportAllDeclaration,
  N_Identifier, N_StringLiteral, N_NumericLiteral, N_BigIntLiteral, N_BooleanLiteral, N_NullLiteral,
  N_RegExpLiteral, N_TemplateLiteral, N_TemplateElement, N_TaggedTemplateExpression,
  N_ThisExpression, N_Super, N_Import, N_MetaProperty,
  N_ArrayExpression, N_ObjectExpression, N_ObjectProperty, N_ObjectMethod, N_SpreadElement, N_RestElement,
  N_FunctionExpression, N_ArrowFunctionExpression,
  N_UnaryExpression, N_UpdateExpression, N_BinaryExpression, N_LogicalExpression, N_AssignmentExpression,
  N_ConditionalExpression, N_CallExpression, N_NewExpression, N_MemberExpression,
  N_OptionalMemberExpression, N_OptionalCallExpression, N_SequenceExpression, N_YieldExpression,
  N_AwaitExpression, N_AssignmentPattern, N_ArrayPattern, N_ObjectPattern,
  N_ClassProperty   /* appended: class fields (key, optional initializer in b) */
};

enum { JF_COMPUTED = 1, JF_STATIC = 2, JF_ASYNC = 4, JF_GENERATOR = 8, JF_SHORTHAND = 16,
       JF_PREFIX = 32, JF_OPTIONAL = 64, JF_DELEGATE = 128, JF_EXPR_BODY = 256 };

/* Field use by node type (a, b, c, d are child slots; list and list2 are child arrays):
 *   Program: list body, list2 directives. BlockStatement: list body, list2 directives (function
 *   bodies only). Identifier/literals: str,len = source text. VariableDeclaration: op = 0 var,
 *   1 let, 2 const; list declarators. Binary/Logical/Assignment/Unary/Update: op = token,
 *   a, b operands. Member: a object, b property. Call/New: a callee, list arguments.
 *   Function/Arrow/ClassMethod/ObjectMethod: a = id or key, b = body, list = params;
 *   op = method kind for class and object methods (0 method, 1 get, 2 set, 3 constructor).
 *   Class: a id, b superClass, c body (ClassBody with list members).
 *   Template: list interleaves TemplateElement and expressions. */
typedef struct JsNode {
  uint8_t type, op;
  uint16_t flags;
  uint32_t start, end;
  const char *str; uint32_t len;
  struct JsNode *a, *b, *c, *d;
  struct JsNode **list; uint32_t nlist;
  struct JsNode **list2; uint32_t nlist2;
} JsNode;

/* ---- tokens ---- */
enum JsTok {
  TK_EOF, TK_NAME, TK_NUM, TK_BIGINT, TK_STRING, TK_REGEX, TK_TEMPLATE,
  TK_BREAK, TK_CASE, TK_CATCH, TK_CLASS, TK_CONST, TK_CONTINUE, TK_DEBUGGER, TK_DEFAULT, TK_DELETE,
  TK_DO, TK_ELSE, TK_EXPORT, TK_EXTENDS, TK_FINALLY, TK_FOR, TK_FUNCTION, TK_IF, TK_IMPORT, TK_IN,
  TK_INSTANCEOF, TK_NEW, TK_RETURN, TK_SUPER, TK_SWITCH, TK_THIS, TK_THROW, TK_TRY, TK_TYPEOF,
  TK_VAR, TK_VOID, TK_WHILE, TK_WITH, TK_NULL, TK_TRUE, TK_FALSE,
  TK_LBRACE, TK_RBRACE, TK_LPAREN, TK_RPAREN, TK_LBRACK, TK_RBRACK, TK_DOT, TK_ELLIPSIS, TK_SEMI,
  TK_COMMA, TK_LT, TK_GT, TK_LE, TK_GE, TK_EQ, TK_NE, TK_SEQ, TK_SNE, TK_PLUS, TK_MINUS, TK_STAR,
  TK_SLASH, TK_PERCENT, TK_STARSTAR, TK_INC, TK_DEC, TK_SHL, TK_SAR, TK_SHR, TK_AMP, TK_BAR,
  TK_CARET, TK_BANG, TK_TILDE, TK_AND, TK_OR, TK_NULLISH, TK_QUESTION, TK_QDOT, TK_COLON,
  TK_ASSIGN, TK_PLUSEQ, TK_MINUSEQ, TK_STAREQ, TK_SLASHEQ, TK_PERCENTEQ, TK_STARSTAREQ, TK_SHLEQ,
  TK_SAREQ, TK_SHREQ, TK_AMPEQ, TK_BAREQ, TK_CARETEQ, TK_ANDEQ, TK_OREQ, TK_NULLISHEQ, TK_ARROW,
  TK_BACKQUOTE
};

typedef struct {
  size_t pos, ts, te, prev_end, ntok;
  int tt, nl;
} JsLexState;

typedef struct JsContext {
  JsArena ar;
  JsNode **stk; size_t sp, scap;
  /* parser state for the current parse */
  const char *src; size_t len; size_t pos, ts, te, prev_end, ntok;
  int tt, nl, module, in_function, in_async, in_generator;
} JsContext;

static void js_fail(const JsContext *P, const char *msg) {
  size_t line = 1, lstart = 0;
  for (size_t i = 0; i < P->ts && i < P->len; i++)
    if (P->src[i] == '\n') { line++; lstart = i + 1; }
  fprintf(stderr, "js-parser: %s at line %zu, column %zu (token %d)\n", msg, line, P->ts - lstart + 1, P->tt);
  exit(3);
}

/* ---- arena ---- */
static JsChunk *js_chunk_new(size_t cap) {
  JsChunk *c = malloc(sizeof *c + cap);
  if (!c) { fprintf(stderr, "js-parser: out of memory\n"); exit(3); }
  c->next = NULL; c->cap = cap; c->used = 0;
  return c;
}
static void *js_alloc(JsContext *P, size_t n) {
  n = (n + 15) & ~(size_t)15;
  JsArena *a = &P->ar;
  if (!a->cur) { a->head = a->cur = js_chunk_new(n > (1u << 20) ? n : (1u << 20)); }
  for (;;) {
    if (a->cur->used + n <= a->cur->cap) {
      void *p = a->cur->data + a->cur->used;
      a->cur->used += n;
      return p;
    }
    if (a->cur->next && a->cur->next->cap >= n) { a->cur = a->cur->next; continue; }
    JsChunk *c = js_chunk_new(n > (1u << 20) ? n : (1u << 20));
    c->next = a->cur->next;
    a->cur->next = c;
    a->cur = c;
  }
}
static void js_arena_reset(JsArena *a) {
  for (JsChunk *c = a->head; c; c = c->next) c->used = 0;
  a->cur = a->head;
}
static void js_context_init(JsContext *c) { memset(c, 0, sizeof *c); }
static void js_context_destroy(JsContext *c) {
  for (JsChunk *ch = c->ar.head; ch;) { JsChunk *n = ch->next; free(ch); ch = n; }
  free(c->stk);
  js_context_init(c);
}

/* ---- node and list helpers ---- */
static JsNode *js_node(JsContext *P, int type, size_t start) {
  JsNode *n = js_alloc(P, sizeof *n);
  memset(n, 0, sizeof *n);
  n->type = (uint8_t)type;
  n->start = (uint32_t)start;
  return n;
}
static JsNode *js_fin(JsContext *P, JsNode *n) { n->end = (uint32_t)P->prev_end; return n; }
static void js_push(JsContext *P, JsNode *n) {
  if (P->sp == P->scap) {
    P->scap = P->scap ? P->scap * 2 : 1024;
    P->stk = realloc(P->stk, P->scap * sizeof *P->stk);
    if (!P->stk) { fprintf(stderr, "js-parser: out of memory\n"); exit(3); }
  }
  P->stk[P->sp++] = n;
}
static void js_take_list(JsContext *P, size_t mark, JsNode ***out, uint32_t *nout) {
  size_t k = P->sp - mark;
  JsNode **arr = NULL;
  if (k) {
    arr = js_alloc(P, k * sizeof *arr);
    memcpy(arr, P->stk + mark, k * sizeof *arr);
  }
  P->sp = mark;
  *out = arr;
  *nout = (uint32_t)k;
}

/* ---- lexer ---- */
static void js_save(const JsContext *P, JsLexState *s) {
  s->pos = P->pos; s->ts = P->ts; s->te = P->te; s->prev_end = P->prev_end; s->ntok = P->ntok;
  s->tt = P->tt; s->nl = P->nl;
}
static void js_restore(JsContext *P, const JsLexState *s) {
  P->pos = s->pos; P->ts = s->ts; P->te = s->te; P->prev_end = s->prev_end; P->ntok = s->ntok;
  P->tt = s->tt; P->nl = s->nl;
}
/* Character classes: one table lookup per character instead of range tests. */
enum { CC_IDSTART = 1, CC_IDPART = 2 };
static unsigned char js_cls[256];
static int js_cls_ready;
static void js_init_classes(void) {
  for (unsigned c = 0; c < 256; c++) {
    unsigned char v = 0;
    if ((c | 0x20) - 'a' < 26u || c == '_' || c == '$') v = CC_IDSTART | CC_IDPART;
    else if (c - '0' < 10u) v = CC_IDPART;
    js_cls[c] = v;
  }
  js_cls_ready = 1;
}
static inline int js_ident_start(unsigned c) { return js_cls[c & 0xFF] & CC_IDSTART; }
static inline int js_ident_part(unsigned c) { return js_cls[c & 0xFF] & CC_IDPART; }

/* Skips whitespace and comments from index i; sets *nl when a line terminator is passed. */
static size_t js_skip(JsContext *P, size_t i, int *nl) {
  const unsigned char *s = (const unsigned char *)P->src;
  const size_t n = P->len;
  while (i < n) {
    unsigned char c = s[i];
    if (c == ' ' || c == '\t' || c == '\v' || c == '\f') i++;
    else if (c == '\n' || c == '\r') { *nl = 1; i++; }
    else if (c == '/' && i + 1 < n && s[i + 1] == '/') {
      i += 2;
      while (i < n && s[i] != '\n' && s[i] != '\r') i++;
    } else if (c == '/' && i + 1 < n && s[i + 1] == '*') {
      size_t e = i + 2;
      while (e + 1 < n && !(s[e] == '*' && s[e + 1] == '/')) { if (s[e] == '\n') *nl = 1; e++; }
      if (e + 1 >= n) { P->ts = i; js_fail(P, "unterminated comment"); }
      i = e + 2;
    } else if (c == 0xC2 && i + 1 < n && s[i + 1] == 0xA0) i += 2;
    else if (c == 0xEF && i + 2 < n && s[i + 1] == 0xBB && s[i + 2] == 0xBF) i += 3;
    else if (c == 0xE2 && i + 2 < n && s[i + 1] == 0x80 && (s[i + 2] == 0xA8 || s[i + 2] == 0xA9)) { *nl = 1; i += 3; }
    else if (c == '#' && i == 0 && n > 1 && s[1] == '!') { while (i < n && s[i] != '\n') i++; }
    else break;
  }
  return i;
}

static int js_keyword(const char *s, size_t n) {
#define JS_KW(lit, t) if (n == sizeof(lit) - 1 && memcmp(s, lit, sizeof(lit) - 1) == 0) return t
  if (n < 2 || n > 10 || s[0] < 'a' || s[0] > 'z') return TK_NAME;
  switch (s[0]) {
  case 'b': JS_KW("break", TK_BREAK); break;
  case 'c': JS_KW("case", TK_CASE); JS_KW("catch", TK_CATCH); JS_KW("class", TK_CLASS);
            JS_KW("const", TK_CONST); JS_KW("continue", TK_CONTINUE); break;
  case 'd': JS_KW("debugger", TK_DEBUGGER); JS_KW("default", TK_DEFAULT); JS_KW("delete", TK_DELETE);
            JS_KW("do", TK_DO); break;
  case 'e': JS_KW("else", TK_ELSE); JS_KW("export", TK_EXPORT); JS_KW("extends", TK_EXTENDS); break;
  case 'f': JS_KW("finally", TK_FINALLY); JS_KW("for", TK_FOR); JS_KW("function", TK_FUNCTION);
            JS_KW("false", TK_FALSE); break;
  case 'i': JS_KW("if", TK_IF); JS_KW("import", TK_IMPORT); JS_KW("in", TK_IN);
            JS_KW("instanceof", TK_INSTANCEOF); break;
  case 'n': JS_KW("new", TK_NEW); JS_KW("null", TK_NULL); break;
  case 'r': JS_KW("return", TK_RETURN); break;
  case 's': JS_KW("super", TK_SUPER); JS_KW("switch", TK_SWITCH); break;
  case 't': JS_KW("this", TK_THIS); JS_KW("throw", TK_THROW); JS_KW("try", TK_TRY);
            JS_KW("typeof", TK_TYPEOF); JS_KW("true", TK_TRUE); break;
  case 'v': JS_KW("var", TK_VAR); JS_KW("void", TK_VOID); break;
  case 'w': JS_KW("while", TK_WHILE); JS_KW("with", TK_WITH); break;
  }
#undef JS_KW
  return TK_NAME;
}

static void js_scan_number(JsContext *P, size_t i, int *tt, size_t *end) {
  const unsigned char *s = (const unsigned char *)P->src;
  size_t n = P->len;
  if (s[i] == '0' && i + 1 < n && (s[i + 1] | 0x20) == 'x') {
    i += 2; while (i < n && (isxdigit(s[i]) || s[i] == '_')) i++;
  } else if (s[i] == '0' && i + 1 < n && ((s[i + 1] | 0x20) == 'o' || (s[i + 1] | 0x20) == 'b')) {
    i += 2; while (i < n && ((s[i] >= '0' && s[i] <= '9') || s[i] == '_')) i++;
  } else {
    while (i < n && ((s[i] >= '0' && s[i] <= '9') || s[i] == '_')) i++;
    if (i < n && s[i] == '.') { i++; while (i < n && ((s[i] >= '0' && s[i] <= '9') || s[i] == '_')) i++; }
    if (i < n && (s[i] | 0x20) == 'e') {
      i++;
      if (i < n && (s[i] == '+' || s[i] == '-')) i++;
      while (i < n && s[i] >= '0' && s[i] <= '9') i++;
    }
  }
  *tt = TK_NUM;
  if (i < n && s[i] == 'n') { *tt = TK_BIGINT; i++; }
  if (i < n && js_ident_part(s[i])) js_fail(P, "identifier directly after number");
  *end = i;
}

static void js_lex(JsContext *P) {
  const unsigned char *s = (const unsigned char *)P->src;
  const size_t n = P->len;
  size_t i;
  int nl = 0;
  P->prev_end = P->te;
  i = P->pos;
  /* fast path: most tokens follow a token with no space in between */
  if (i < n && s[i] > ' ' && s[i] < 0x80 &&
      !(s[i] == '/' && i + 1 < n && (s[i + 1] == '/' || s[i + 1] == '*')) && s[i] != '#') {
    /* no whitespace or comment before the token */
  } else i = js_skip(P, i, &nl);
  P->nl = nl;
  P->ts = i;
  P->ntok++;
  if (i >= n) { P->tt = TK_EOF; P->te = P->pos = i; return; }
  unsigned char c = s[i];
#define CH(k) (i + (k) < n ? s[i + (k)] : 0)
  unsigned char c1 = CH(1);
  int tt = TK_EOF;
  size_t e = i + 1;
#define TOK(len, t) do { tt = (t); e = i + (len); } while (0)
  if (js_ident_start(c) || (c == '\\' && c1 == 'u')) {
    /* identifier; \uXXXX escapes are kept in the token text (a name such as a\u00e9roport) */
    e = i;
    for (;;) {
      if (e < n && js_ident_part(s[e])) e++;
      else if (e + 5 < n && s[e] == '\\' && s[e + 1] == 'u' && isxdigit(s[e + 2]) && isxdigit(s[e + 3]) &&
               isxdigit(s[e + 4]) && isxdigit(s[e + 5])) e += 6;
      else break;
    }
    if (e < n && (s[e] == '\\' || s[e] >= 0x80)) js_fail(P, "unsupported identifier escape or non-ASCII identifier");
    tt = js_keyword(P->src + i, e - i);
  } else if (c == '#' && js_ident_start(c1)) {
    /* private name (#x): lexed as a name token that keeps its '#', so class members and this.#x
     * parse as ordinary member names; the node is not a PrivateName as in Babel */
    e = i + 1;
    while (e < n && js_ident_part(s[e])) e++;
    tt = TK_NAME;
  } else if (c >= '0' && c <= '9') {
    js_scan_number(P, i, &tt, &e);
  } else if (c == '.' && c1 >= '0' && c1 <= '9') {
    js_scan_number(P, i, &tt, &e);
  } else if (c == '"' || c == '\'') {
    e = i + 1;
    while (e < n && s[e] != c) {
      if (s[e] == '\\') e += (e + 2 < n && s[e + 1] == '\r' && s[e + 2] == '\n') ? 3 : 2;
      else if (s[e] == '\n' || s[e] == '\r') js_fail(P, "newline in string literal");
      else e++;
    }
    if (e >= n) js_fail(P, "unterminated string literal");
    e++;
    tt = TK_STRING;
  } else {
    switch (c) {
    case '`': TOK(1, TK_BACKQUOTE); break;
    case '{': TOK(1, TK_LBRACE); break;
    case '}': TOK(1, TK_RBRACE); break;
    case '(': TOK(1, TK_LPAREN); break;
    case ')': TOK(1, TK_RPAREN); break;
    case '[': TOK(1, TK_LBRACK); break;
    case ']': TOK(1, TK_RBRACK); break;
    case ';': TOK(1, TK_SEMI); break;
    case ',': TOK(1, TK_COMMA); break;
    case ':': TOK(1, TK_COLON); break;
    case '~': TOK(1, TK_TILDE); break;
    case '.': if (c1 == '.' && CH(2) == '.') TOK(3, TK_ELLIPSIS); else TOK(1, TK_DOT); break;
    case '?':
      if (c1 == '?') { if (CH(2) == '=') TOK(3, TK_NULLISHEQ); else TOK(2, TK_NULLISH); }
      else if (c1 == '.' && !(CH(2) >= '0' && CH(2) <= '9')) TOK(2, TK_QDOT);
      else TOK(1, TK_QUESTION);
      break;
    case '<':
      if (c1 == '<') { if (CH(2) == '=') TOK(3, TK_SHLEQ); else TOK(2, TK_SHL); }
      else if (c1 == '=') TOK(2, TK_LE); else TOK(1, TK_LT);
      break;
    case '>':
      if (c1 == '>') {
        if (CH(2) == '>') { if (CH(3) == '=') TOK(4, TK_SHREQ); else TOK(3, TK_SHR); }
        else if (CH(2) == '=') TOK(3, TK_SAREQ); else TOK(2, TK_SAR);
      } else if (c1 == '=') TOK(2, TK_GE); else TOK(1, TK_GT);
      break;
    case '=':
      if (c1 == '=') { if (CH(2) == '=') TOK(3, TK_SEQ); else TOK(2, TK_EQ); }
      else if (c1 == '>') TOK(2, TK_ARROW); else TOK(1, TK_ASSIGN);
      break;
    case '!':
      if (c1 == '=') { if (CH(2) == '=') TOK(3, TK_SNE); else TOK(2, TK_NE); } else TOK(1, TK_BANG);
      break;
    case '+': if (c1 == '+') TOK(2, TK_INC); else if (c1 == '=') TOK(2, TK_PLUSEQ); else TOK(1, TK_PLUS); break;
    case '-': if (c1 == '-') TOK(2, TK_DEC); else if (c1 == '=') TOK(2, TK_MINUSEQ); else TOK(1, TK_MINUS); break;
    case '*':
      if (c1 == '*') { if (CH(2) == '=') TOK(3, TK_STARSTAREQ); else TOK(2, TK_STARSTAR); }
      else if (c1 == '=') TOK(2, TK_STAREQ); else TOK(1, TK_STAR);
      break;
    case '/': if (c1 == '=') TOK(2, TK_SLASHEQ); else TOK(1, TK_SLASH); break;
    case '%': if (c1 == '=') TOK(2, TK_PERCENTEQ); else TOK(1, TK_PERCENT); break;
    case '&':
      if (c1 == '&') { if (CH(2) == '=') TOK(3, TK_ANDEQ); else TOK(2, TK_AND); }
      else if (c1 == '=') TOK(2, TK_AMPEQ); else TOK(1, TK_AMP);
      break;
    case '|':
      if (c1 == '|') { if (CH(2) == '=') TOK(3, TK_OREQ); else TOK(2, TK_OR); }
      else if (c1 == '=') TOK(2, TK_BAREQ); else TOK(1, TK_BAR);
      break;
    case '^': if (c1 == '=') TOK(2, TK_CARETEQ); else TOK(1, TK_CARET); break;
    default: P->ts = i; js_fail(P, "unsupported or invalid character");
    }
  }
#undef TOK
#undef CH
  P->tt = tt;
  P->te = e;
  P->pos = e;
}

/* Re-reads the current '/' or '/=' token as a regular expression literal. */
static void js_rescan_regex(JsContext *P) {
  const unsigned char *s = (const unsigned char *)P->src;
  size_t i = P->ts + 1, n = P->len;
  int in_class = 0;
  for (;;) {
    if (i >= n || s[i] == '\n' || s[i] == '\r') js_fail(P, "unterminated regular expression");
    if (s[i] == '\\') { i += 2; continue; }
    if (s[i] == '[') in_class = 1;
    else if (s[i] == ']') in_class = 0;
    else if (s[i] == '/' && !in_class) break;
    i++;
  }
  i++;
  while (i < n && js_ident_part(s[i])) i++;
  P->tt = TK_REGEX;
  P->te = i;
  P->pos = i;
}

/* Scans template text from P->pos. Sets the current token to the chunk and returns 1 when the
 * chunk ends the template (closing backquote), 0 when it ends at a substitution "${". */
static int js_template_chunk(JsContext *P, JsNode *q) {
  const unsigned char *s = (const unsigned char *)P->src;
  size_t i = P->pos, n = P->len, cs = i;
  int tail;
  for (;;) {
    if (i >= n) js_fail(P, "unterminated template");
    if (s[i] == '\\') { i += 2; continue; }
    if (s[i] == '`') { tail = 1; break; }
    if (s[i] == '$' && i + 1 < n && s[i + 1] == '{') { tail = 0; break; }
    i++;
  }
  q->str = P->src + cs;
  q->len = (uint32_t)(i - cs);
  q->end = (uint32_t)i;
  P->ntok += 1;                   /* template token */
  P->tt = TK_TEMPLATE;
  P->ts = cs;
  P->te = i;
  P->pos = i;
  return tail;
}

/* ---- parser helpers ---- */
static int js_is_word(int tt) { return tt == TK_NAME || (tt >= TK_BREAK && tt <= TK_FALSE); }
static int js_is_assign(int tt) {
  return tt == TK_ASSIGN || (tt >= TK_PLUSEQ && tt <= TK_SAREQ) || tt == TK_SHREQ ||
         (tt >= TK_AMPEQ && tt <= TK_NULLISHEQ);
}
static int js_name_is(const JsContext *P, const char *s) {
  size_t n = strlen(s);
  return P->tt == TK_NAME && P->te - P->ts == n && memcmp(P->src + P->ts, s, n) == 0;
}
static void js_expect(JsContext *P, int tt, const char *what) {
  if (P->tt != tt) js_fail(P, what);
  js_lex(P);
}
static void js_consume_semi(JsContext *P) {
  if (P->tt == TK_SEMI) { js_lex(P); return; }
  if (P->tt == TK_RBRACE || P->tt == TK_EOF || P->nl) return;
  js_fail(P, "expected ';'");
}
static int js_peek(JsContext *P, int *nl) {
  JsLexState s; js_save(P, &s);
  js_lex(P);
  int t = P->tt;
  if (nl) *nl = P->nl;
  js_restore(P, &s);
  return t;
}
static JsNode *js_take_ident(JsContext *P) {
  if (P->tt != TK_NAME) js_fail(P, "expected identifier");
  JsNode *n = js_node(P, N_Identifier, P->ts);
  n->str = P->src + P->ts; n->len = (uint32_t)(P->te - P->ts);
  js_lex(P);
  return js_fin(P, n);
}
static JsNode *js_take_word(JsContext *P) {
  if (!js_is_word(P->tt)) js_fail(P, "expected property name");
  JsNode *n = js_node(P, N_Identifier, P->ts);
  n->str = P->src + P->ts; n->len = (uint32_t)(P->te - P->ts);
  js_lex(P);
  return js_fin(P, n);
}
static JsNode *js_take_literal(JsContext *P, int type) {
  JsNode *n = js_node(P, type, P->ts);
  n->str = P->src + P->ts; n->len = (uint32_t)(P->te - P->ts);
  js_lex(P);
  return js_fin(P, n);
}
static JsNode *js_clone_ident(JsContext *P, const JsNode *k) {
  JsNode *n = js_node(P, N_Identifier, k->start);
  n->str = k->str; n->len = k->len; n->end = k->end;
  return n;
}

/* Converts a cover expression to a pattern in place (assignment targets and arrow params). */
static JsNode *js_to_pattern(JsContext *P, JsNode *n) {
  switch (n->type) {
  case N_Identifier: case N_MemberExpression: case N_OptionalMemberExpression:
  case N_AssignmentPattern: case N_RestElement: case N_ObjectPattern: case N_ArrayPattern:
    return n;
  case N_ObjectExpression:
    n->type = N_ObjectPattern;
    for (uint32_t i = 0; i < n->nlist; i++) {
      JsNode *p = n->list[i];
      if (p->type == N_ObjectProperty) p->b = js_to_pattern(P, p->b);
      else if (p->type == N_SpreadElement) { p->type = N_RestElement; p->a = js_to_pattern(P, p->a); }
      else js_fail(P, "invalid destructuring target");
    }
    return n;
  case N_ArrayExpression:
    n->type = N_ArrayPattern;
    for (uint32_t i = 0; i < n->nlist; i++) {
      JsNode *e = n->list[i];
      if (!e) continue;
      if (e->type == N_SpreadElement) { e->type = N_RestElement; e->a = js_to_pattern(P, e->a); }
      else n->list[i] = js_to_pattern(P, e);
    }
    return n;
  case N_SpreadElement:
    n->type = N_RestElement;
    n->a = js_to_pattern(P, n->a);
    return n;
  case N_AssignmentExpression:
    if (n->op != TK_ASSIGN) js_fail(P, "invalid destructuring default");
    n->type = N_AssignmentPattern;
    return n;
  default:
    js_fail(P, "invalid assignment or parameter target");
  }
  return n;
}

static JsNode *js_parse_expression(JsContext *P, int noin);
static JsNode *js_parse_assign(JsContext *P, int noin);
static JsNode *js_parse_binding_atom(JsContext *P);
static JsNode *js_parse_binding_elem(JsContext *P);
static JsNode *js_parse_statement(JsContext *P);
static JsNode *js_parse_statement_list_item(JsContext *P);
static JsNode *js_parse_block(JsContext *P, int directives);
static JsNode *js_parse_class(JsContext *P, size_t start, int is_decl);
static JsNode *js_parse_primary(JsContext *P);
static JsNode *js_parse_template(JsContext *P, size_t start);
static JsNode *js_parse_subscripts(JsContext *P, JsNode *e, int allow_call);

/* Parses a function's parameter list and body into n (params in n->list, body in n->b). */
static void js_parse_fn_rest(JsContext *P, JsNode *n, int async, int gen) {
  int sa = P->in_async, sg = P->in_generator, sf = P->in_function;
  P->in_async = async; P->in_generator = gen; P->in_function = 1;
  js_expect(P, TK_LPAREN, "expected '('");
  size_t mark = P->sp;
  while (P->tt != TK_RPAREN) {
    if (P->tt == TK_ELLIPSIS) {
      JsNode *r = js_node(P, N_RestElement, P->ts);
      js_lex(P);
      r->a = js_parse_binding_atom(P);
      js_push(P, js_fin(P, r));
    } else js_push(P, js_parse_binding_elem(P));
    if (P->tt != TK_RPAREN) js_expect(P, TK_COMMA, "expected ',' in parameters");
  }
  js_lex(P);
  js_take_list(P, mark, &n->list, &n->nlist);
  n->b = js_parse_block(P, 1);
  P->in_async = sa; P->in_generator = sg; P->in_function = sf;
}

/* Statement list with an optional directive prologue; directives go to owner->list2. */
static void js_parse_stmt_list(JsContext *P, int term, JsNode *owner, int directives) {
  size_t mark = P->sp, ndir = 0;
  int prologue = directives;
  while (P->tt != term) {
    if (P->tt == TK_EOF) js_fail(P, "unexpected end of input");
    JsNode *st = js_parse_statement_list_item(P);
    if (prologue && st->type == N_ExpressionStatement && st->a->type == N_StringLiteral &&
        st->a->start == st->start) {
      JsNode *lit = js_node(P, N_DirectiveLiteral, st->a->start);
      lit->str = st->a->str; lit->len = st->a->len; lit->end = st->a->end;
      JsNode *d = js_node(P, N_Directive, st->start);
      d->a = lit; d->end = st->end;
      js_push(P, d);
      ndir++;
    } else {
      prologue = 0;
      js_push(P, st);
    }
  }
  size_t total = P->sp - mark;
  JsNode **dirs = NULL, **body = NULL;
  if (ndir) { dirs = js_alloc(P, ndir * sizeof *dirs); memcpy(dirs, P->stk + mark, ndir * sizeof *dirs); }
  if (total > ndir) {
    body = js_alloc(P, (total - ndir) * sizeof *body);
    memcpy(body, P->stk + mark + ndir, (total - ndir) * sizeof *body);
  }
  P->sp = mark;
  owner->list = body; owner->nlist = (uint32_t)(total - ndir);
  owner->list2 = dirs; owner->nlist2 = (uint32_t)ndir;
}

static JsNode *js_parse_block(JsContext *P, int directives) {
  JsNode *b = js_node(P, N_BlockStatement, P->ts);
  js_expect(P, TK_LBRACE, "expected '{'");
  js_parse_stmt_list(P, TK_RBRACE, b, directives);
  js_lex(P);
  return js_fin(P, b);
}

static JsNode *js_parse_var(JsContext *P, size_t start, int kind, int noin) {
  JsNode *n = js_node(P, N_VariableDeclaration, start);
  size_t mark = P->sp;
  n->op = (uint8_t)kind;
  for (;;) {
    JsNode *d = js_node(P, N_VariableDeclarator, P->ts);
    d->a = js_parse_binding_atom(P);
    if (P->tt == TK_ASSIGN) { js_lex(P); d->b = js_parse_assign(P, noin); }
    js_push(P, js_fin(P, d));
    if (P->tt != TK_COMMA) break;
    js_lex(P);
  }
  js_take_list(P, mark, &n->list, &n->nlist);
  return n;
}

static JsNode *js_parse_binding_atom(JsContext *P) {
  size_t start = P->ts;
  if (P->tt == TK_NAME) return js_take_ident(P);
  if (P->tt == TK_LBRACK) {
    JsNode *n = js_node(P, N_ArrayPattern, start);
    size_t mark = P->sp;
    js_lex(P);
    while (P->tt != TK_RBRACK) {
      if (P->tt == TK_COMMA) { js_push(P, NULL); js_lex(P); continue; }
      if (P->tt == TK_ELLIPSIS) {
        JsNode *r = js_node(P, N_RestElement, P->ts);
        js_lex(P);
        r->a = js_parse_binding_atom(P);
        js_push(P, js_fin(P, r));
      } else js_push(P, js_parse_binding_elem(P));
      if (P->tt != TK_RBRACK) js_expect(P, TK_COMMA, "expected ',' in array pattern");
    }
    js_lex(P);
    js_take_list(P, mark, &n->list, &n->nlist);
    return js_fin(P, n);
  }
  if (P->tt == TK_LBRACE) {
    JsNode *n = js_node(P, N_ObjectPattern, start);
    size_t mark = P->sp;
    js_lex(P);
    while (P->tt != TK_RBRACE) {
      size_t s = P->ts;
      if (P->tt == TK_ELLIPSIS) {
        JsNode *r = js_node(P, N_RestElement, s);
        js_lex(P);
        r->a = js_parse_binding_atom(P);
        js_push(P, js_fin(P, r));
      } else {
        int computed = 0;
        JsNode *key = NULL;
        if (P->tt == TK_LBRACK) {
          js_lex(P); key = js_parse_assign(P, 0); js_expect(P, TK_RBRACK, "expected ']'"); computed = 1;
        } else if (js_is_word(P->tt)) key = js_take_word(P);
        else if (P->tt == TK_STRING) key = js_take_literal(P, N_StringLiteral);
        else if (P->tt == TK_NUM) key = js_take_literal(P, N_NumericLiteral);
        else js_fail(P, "invalid property name in object pattern");
        JsNode *p = js_node(P, N_ObjectProperty, s);
        p->a = key;
        if (computed) p->flags |= JF_COMPUTED;
        if (P->tt == TK_COLON) {
          js_lex(P);
          p->b = js_parse_binding_elem(P);
        } else {
          if (computed || key->type != N_Identifier) js_fail(P, "invalid shorthand property in pattern");
          JsNode *val = js_clone_ident(P, key);
          if (P->tt == TK_ASSIGN) {
            js_lex(P);
            JsNode *ap = js_node(P, N_AssignmentPattern, key->start);
            ap->a = val;
            ap->b = js_parse_assign(P, 0);
            val = js_fin(P, ap);
          }
          p->b = val;
          p->flags |= JF_SHORTHAND;
        }
        js_push(P, js_fin(P, p));
      }
      if (P->tt != TK_RBRACE) js_expect(P, TK_COMMA, "expected ',' in object pattern");
    }
    js_lex(P);
    js_take_list(P, mark, &n->list, &n->nlist);
    return js_fin(P, n);
  }
  js_fail(P, "expected binding pattern");
  return NULL;
}

static JsNode *js_parse_binding_elem(JsContext *P) {
  size_t start = P->ts;
  JsNode *a = js_parse_binding_atom(P);
  if (P->tt != TK_ASSIGN) return a;
  js_lex(P);
  JsNode *n = js_node(P, N_AssignmentPattern, start);
  n->a = a;
  n->b = js_parse_assign(P, 0);
  return js_fin(P, n);
}

static JsNode *js_parse_prop_key(JsContext *P, int *computed) {
  *computed = 0;
  if (P->tt == TK_LBRACK) {
    js_lex(P);
    JsNode *k = js_parse_assign(P, 0);
    js_expect(P, TK_RBRACK, "expected ']'");
    *computed = 1;
    return k;
  }
  if (js_is_word(P->tt)) return js_take_word(P);
  if (P->tt == TK_STRING) return js_take_literal(P, N_StringLiteral);
  if (P->tt == TK_NUM) return js_take_literal(P, N_NumericLiteral);
  if (P->tt == TK_BIGINT) return js_take_literal(P, N_BigIntLiteral);
  js_fail(P, "invalid property name");
  return NULL;
}

/* Modifiers of object and class members: static, async, '*', get, set. */
typedef struct { int kind, is_async, is_gen, is_static; } JsMemberMods;
static int js_mod_follows_ok(int t) {
  return !(t == TK_LPAREN || t == TK_ASSIGN || t == TK_SEMI || t == TK_RBRACE || t == TK_COMMA ||
           t == TK_COLON || t == TK_EOF);
}
static void js_member_mods(JsContext *P, int allow_static, JsMemberMods *m) {
  int nl = 0, t;
  memset(m, 0, sizeof *m);
  if (allow_static && js_name_is(P, "static") ) {
    t = js_peek(P, NULL);
    if (js_mod_follows_ok(t)) { m->is_static = 1; js_lex(P); }
  }
  if (js_name_is(P, "async")) {
    t = js_peek(P, &nl);
    if (js_mod_follows_ok(t) && !nl) { m->is_async = 1; js_lex(P); }
  }
  if (P->tt == TK_STAR) { m->is_gen = 1; js_lex(P); }
  if (js_name_is(P, "get") || js_name_is(P, "set")) {
    t = js_peek(P, NULL);
    if (js_mod_follows_ok(t)) { m->kind = js_name_is(P, "get") ? 1 : 2; js_lex(P); }
  }
}

static JsNode *js_parse_object_member(JsContext *P) {
  size_t start = P->ts;
  if (P->tt == TK_ELLIPSIS) {
    JsNode *sp = js_node(P, N_SpreadElement, start);
    js_lex(P);
    sp->a = js_parse_assign(P, 0);
    return js_fin(P, sp);
  }
  JsMemberMods m;
  js_member_mods(P, 0, &m);
  int computed;
  JsNode *key = js_parse_prop_key(P, &computed);
  if (P->tt == TK_LPAREN || m.kind || m.is_async || m.is_gen) {
    JsNode *fn = js_node(P, N_ObjectMethod, start);
    fn->a = key; fn->op = (uint8_t)m.kind;
    if (computed) fn->flags |= JF_COMPUTED;
    if (m.is_async) fn->flags |= JF_ASYNC;
    if (m.is_gen) fn->flags |= JF_GENERATOR;
    js_parse_fn_rest(P, fn, m.is_async, m.is_gen);
    return js_fin(P, fn);
  }
  JsNode *p = js_node(P, N_ObjectProperty, start);
  p->a = key;
  if (computed) p->flags |= JF_COMPUTED;
  if (P->tt == TK_COLON) {
    js_lex(P);
    p->b = js_parse_assign(P, 0);
    return js_fin(P, p);
  }
  if (computed || key->type != N_Identifier) js_fail(P, "invalid shorthand property");
  JsNode *val = js_clone_ident(P, key);
  if (P->tt == TK_ASSIGN) {         /* cover grammar: only valid as a pattern */
    js_lex(P);
    JsNode *ap = js_node(P, N_AssignmentPattern, key->start);
    ap->a = val;
    ap->b = js_parse_assign(P, 0);
    val = js_fin(P, ap);
  }
  p->b = val;
  p->flags |= JF_SHORTHAND;
  return js_fin(P, p);
}

static JsNode *js_parse_object(JsContext *P) {
  JsNode *n = js_node(P, N_ObjectExpression, P->ts);
  size_t mark = P->sp;
  js_lex(P);
  while (P->tt != TK_RBRACE) {
    js_push(P, js_parse_object_member(P));
    if (P->tt != TK_RBRACE) js_expect(P, TK_COMMA, "expected ',' in object literal");
  }
  js_lex(P);
  js_take_list(P, mark, &n->list, &n->nlist);
  return js_fin(P, n);
}

static JsNode *js_parse_class_member(JsContext *P) {
  size_t start = P->ts;
  JsMemberMods m;
  js_member_mods(P, 1, &m);
  int computed;
  JsNode *key = js_parse_prop_key(P, &computed);
  if (P->tt != TK_LPAREN) {
    /* a class field: key [= initializer] ; (ASI applies) */
    JsNode *f = js_node(P, N_ClassProperty, start);
    f->a = key;
    if (computed) f->flags |= JF_COMPUTED;
    if (m.is_static) f->flags |= JF_STATIC;
    if (P->tt == TK_ASSIGN) { js_lex(P); f->b = js_parse_assign(P, 0); }
    js_consume_semi(P);
    return js_fin(P, f);
  }
  if (!m.kind && !computed && !m.is_static && key->type == N_Identifier && key->len == 11 &&
      memcmp(key->str, "constructor", 11) == 0)
    m.kind = 3;
  JsNode *fn = js_node(P, N_ClassMethod, start);
  fn->a = key; fn->op = (uint8_t)m.kind;
  if (computed) fn->flags |= JF_COMPUTED;
  if (m.is_static) fn->flags |= JF_STATIC;
  if (m.is_async) fn->flags |= JF_ASYNC;
  if (m.is_gen) fn->flags |= JF_GENERATOR;
  js_parse_fn_rest(P, fn, m.is_async, m.is_gen);
  return js_fin(P, fn);
}

static JsNode *js_parse_class(JsContext *P, size_t start, int is_decl) {
  js_lex(P);                                  /* 'class' */
  JsNode *id = NULL, *sup = NULL;
  if (P->tt == TK_NAME) id = js_take_ident(P);
  if (P->tt == TK_EXTENDS) { js_lex(P); sup = js_parse_subscripts(P, js_parse_primary(P), 1); }
  JsNode *body = js_node(P, N_ClassBody, P->ts);
  size_t mark = P->sp;
  js_expect(P, TK_LBRACE, "expected '{' in class body");
  while (P->tt != TK_RBRACE) {
    if (P->tt == TK_SEMI) { js_lex(P); continue; }
    js_push(P, js_parse_class_member(P));
  }
  js_lex(P);
  js_take_list(P, mark, &body->list, &body->nlist);
  js_fin(P, body);
  JsNode *n = js_node(P, is_decl ? N_ClassDeclaration : N_ClassExpression, start);
  n->a = id; n->b = sup; n->c = body;
  return js_fin(P, n);
}

/* ---- expressions ---- */
static JsNode **js_one(JsContext *P, JsNode *n) {
  JsNode **a = js_alloc(P, sizeof *a);
  a[0] = n;
  return a;
}

static JsNode *js_parse_arrow(JsContext *P, size_t start, JsNode **params, uint32_t np, int async) {
  int sa = P->in_async, sg = P->in_generator, sf = P->in_function;
  js_lex(P);                                  /* '=>' */
  P->in_async = async; P->in_generator = 0; P->in_function = 1;
  JsNode *n = js_node(P, N_ArrowFunctionExpression, start);
  n->list = params; n->nlist = np;
  if (async) n->flags |= JF_ASYNC;
  if (P->tt == TK_LBRACE) n->a = js_parse_block(P, 1);
  else { n->a = js_parse_assign(P, 0); n->flags |= JF_EXPR_BODY; }
  P->in_async = sa; P->in_generator = sg; P->in_function = sf;
  return js_fin(P, n);
}

static void js_parse_args(JsContext *P, JsNode *n) {
  size_t mark = P->sp;
  js_lex(P);                                  /* '(' */
  while (P->tt != TK_RPAREN) {
    if (P->tt == TK_ELLIPSIS) {
      JsNode *sp = js_node(P, N_SpreadElement, P->ts);
      js_lex(P);
      sp->a = js_parse_assign(P, 0);
      js_push(P, js_fin(P, sp));
    } else js_push(P, js_parse_assign(P, 0));
    if (P->tt != TK_RPAREN) js_expect(P, TK_COMMA, "expected ',' in arguments");
  }
  js_lex(P);
  js_take_list(P, mark, &n->list, &n->nlist);
}

static JsNode *js_parse_new(JsContext *P) {
  size_t start = P->ts;
  js_lex(P);                                  /* 'new' */
  if (P->tt == TK_DOT) {
    JsNode *meta = js_node(P, N_Identifier, start);
    meta->str = P->src + start; meta->len = 3; meta->end = (uint32_t)(start + 3);
    js_lex(P);
    if (!js_name_is(P, "target")) js_fail(P, "expected new.target");
    JsNode *mp = js_node(P, N_MetaProperty, start);
    mp->a = meta; mp->b = js_take_word(P);
    return js_fin(P, mp);
  }
  JsNode *callee = (P->tt == TK_NEW) ? js_parse_new(P) : js_parse_primary(P);
  callee = js_parse_subscripts(P, callee, 0);
  JsNode *n = js_node(P, N_NewExpression, start);
  n->a = callee;
  if (P->tt == TK_LPAREN) js_parse_args(P, n);
  return js_fin(P, n);
}

static JsNode *js_parse_subscripts(JsContext *P, JsNode *e, int allow_call) {
  int chain = 0;
  for (;;) {
    size_t start = e->start;
    if (P->tt == TK_DOT) {
      js_lex(P);
      JsNode *n = js_node(P, chain ? N_OptionalMemberExpression : N_MemberExpression, start);
      n->a = e; n->b = js_take_word(P);
      e = js_fin(P, n);
    } else if (P->tt == TK_QDOT) {
      chain = 1;
      js_lex(P);
      if (P->tt == TK_LPAREN && allow_call) {
        JsNode *n = js_node(P, N_OptionalCallExpression, start);
        n->a = e; n->flags |= JF_OPTIONAL;
        js_parse_args(P, n);
        e = js_fin(P, n);
      } else if (P->tt == TK_LBRACK) {
        js_lex(P);
        JsNode *n = js_node(P, N_OptionalMemberExpression, start);
        n->a = e; n->flags |= JF_COMPUTED | JF_OPTIONAL;
        n->b = js_parse_expression(P, 0);
        js_expect(P, TK_RBRACK, "expected ']'");
        e = js_fin(P, n);
      } else {
        JsNode *n = js_node(P, N_OptionalMemberExpression, start);
        n->a = e; n->flags |= JF_OPTIONAL;
        n->b = js_take_word(P);
        e = js_fin(P, n);
      }
    } else if (P->tt == TK_LBRACK) {
      js_lex(P);
      JsNode *n = js_node(P, chain ? N_OptionalMemberExpression : N_MemberExpression, start);
      n->a = e; n->flags |= JF_COMPUTED;
      n->b = js_parse_expression(P, 0);
      js_expect(P, TK_RBRACK, "expected ']'");
      e = js_fin(P, n);
    } else if (P->tt == TK_LPAREN && allow_call) {
      JsNode *n = js_node(P, chain ? N_OptionalCallExpression : N_CallExpression, start);
      n->a = e;
      js_parse_args(P, n);
      e = js_fin(P, n);
    } else if (P->tt == TK_BACKQUOTE) {
      if (chain) js_fail(P, "tagged template in optional chain");
      JsNode *n = js_node(P, N_TaggedTemplateExpression, start);
      n->a = e;
      n->b = js_parse_template(P, P->ts);
      e = js_fin(P, n);
    } else break;
  }
  return e;
}

static JsNode *js_parse_template(JsContext *P, size_t start) {
  /* current token is the opening backquote; P->pos is just after it */
  JsNode *n = js_node(P, N_TemplateLiteral, start);
  size_t mark = P->sp;
  for (;;) {
    JsNode *q = js_node(P, N_TemplateElement, P->pos);
    int tail = js_template_chunk(P, q);
    js_push(P, q);
    if (tail) {
      P->ntok += 1;                           /* closing backquote */
      P->te = P->pos + 1; P->pos = P->pos + 1;
      js_lex(P);
      break;
    }
    P->ntok += 1;                             /* "${" */
    P->pos = P->te + 2; P->te = P->pos;
    js_lex(P);
    js_push(P, js_parse_expression(P, 0));
    if (P->tt != TK_RBRACE) js_fail(P, "expected '}' in template substitution");
    P->pos = P->te;                           /* scan the next chunk right after '}' */
  }
  js_take_list(P, mark, &n->list, &n->nlist);
  return js_fin(P, n);
}

static JsNode *js_parse_primary(JsContext *P) {
  size_t start = P->ts;
  JsNode *n;
  switch (P->tt) {
  case TK_NAME: {
    if (js_name_is(P, "async")) {
      JsLexState s; js_save(P, &s);
      js_lex(P);
      if (P->tt == TK_FUNCTION && !P->nl) {
        n = js_node(P, N_FunctionExpression, start);
        n->flags |= JF_ASYNC;
        js_lex(P);
        if (P->tt == TK_STAR) { n->flags |= JF_GENERATOR; js_lex(P); }
        if (P->tt == TK_NAME) n->a = js_take_ident(P);
        js_parse_fn_rest(P, n, 1, (n->flags & JF_GENERATOR) != 0);
        return js_fin(P, n);
      }
      if (P->tt == TK_NAME && !P->nl) {
        JsNode *param = js_take_ident(P);
        if (P->tt == TK_ARROW && !P->nl) return js_parse_arrow(P, start, js_one(P, param), 1, 1);
        js_restore(P, &s);
      } else if (P->tt == TK_LPAREN && !P->nl) {
        JsNode *id = js_node(P, N_Identifier, start);
        id->str = P->src + start; id->len = 5; id->end = (uint32_t)(start + 5);
        JsNode *call = js_node(P, N_CallExpression, start);
        call->a = id;
        js_parse_args(P, call);
        if (P->tt == TK_ARROW && !P->nl) {
          for (uint32_t i = 0; i < call->nlist; i++) call->list[i] = js_to_pattern(P, call->list[i]);
          return js_parse_arrow(P, start, call->list, call->nlist, 1);
        }
        return js_fin(P, call);
      } else js_restore(P, &s);
    }
    JsNode *id = js_take_ident(P);
    if (P->tt == TK_ARROW && !P->nl) return js_parse_arrow(P, start, js_one(P, id), 1, 0);
    return id;
  }
  case TK_NUM: return js_take_literal(P, N_NumericLiteral);
  case TK_BIGINT: return js_take_literal(P, N_BigIntLiteral);
  case TK_STRING: return js_take_literal(P, N_StringLiteral);
  case TK_TRUE: case TK_FALSE: return js_take_literal(P, N_BooleanLiteral);
  case TK_NULL: return js_take_literal(P, N_NullLiteral);
  case TK_THIS: n = js_node(P, N_ThisExpression, start); js_lex(P); return js_fin(P, n);
  case TK_SUPER: n = js_node(P, N_Super, start); js_lex(P); return js_fin(P, n);
  case TK_BACKQUOTE: return js_parse_template(P, start);
  case TK_SLASH: case TK_SLASHEQ:
    js_rescan_regex(P);
    n = js_node(P, N_RegExpLiteral, start);
    n->str = P->src + P->ts; n->len = (uint32_t)(P->te - P->ts);
    js_lex(P);
    return js_fin(P, n);
  case TK_LBRACK: {
    n = js_node(P, N_ArrayExpression, start);
    size_t mark = P->sp;
    js_lex(P);
    while (P->tt != TK_RBRACK) {
      if (P->tt == TK_COMMA) { js_push(P, NULL); js_lex(P); continue; }
      if (P->tt == TK_ELLIPSIS) {
        JsNode *sp = js_node(P, N_SpreadElement, P->ts);
        js_lex(P);
        sp->a = js_parse_assign(P, 0);
        js_push(P, js_fin(P, sp));
      } else js_push(P, js_parse_assign(P, 0));
      if (P->tt != TK_RBRACK) js_expect(P, TK_COMMA, "expected ',' in array literal");
    }
    js_lex(P);
    js_take_list(P, mark, &n->list, &n->nlist);
    return js_fin(P, n);
  }
  case TK_LBRACE: return js_parse_object(P);
  case TK_FUNCTION:
    n = js_node(P, N_FunctionExpression, start);
    js_lex(P);
    if (P->tt == TK_STAR) { n->flags |= JF_GENERATOR; js_lex(P); }
    if (P->tt == TK_NAME) n->a = js_take_ident(P);
    js_parse_fn_rest(P, n, 0, (n->flags & JF_GENERATOR) != 0);
    return js_fin(P, n);
  case TK_CLASS: return js_parse_class(P, start, 0);
  case TK_IMPORT:
    js_lex(P);
    if (P->tt == TK_DOT) {
      if (!P->module) js_fail(P, "import.meta outside a module");
      JsNode *meta = js_node(P, N_Identifier, start);
      meta->str = P->src + start; meta->len = 6; meta->end = (uint32_t)(start + 6);
      js_lex(P);
      n = js_node(P, N_MetaProperty, start);
      n->a = meta; n->b = js_take_word(P);
      return js_fin(P, n);
    }
    n = js_node(P, N_Import, start);
    return js_fin(P, n);
  case TK_LPAREN: {
    js_lex(P);
    size_t mark = P->sp;
    int rest = 0, trailing = 0;
    if (P->tt == TK_RPAREN) {
      js_lex(P);
      if (!(P->tt == TK_ARROW && !P->nl)) js_fail(P, "'()' must be an arrow function head");
      return js_parse_arrow(P, start, NULL, 0, 0);
    }
    while (P->tt != TK_RPAREN) {
      if (P->tt == TK_ELLIPSIS) {
        JsNode *r = js_node(P, N_RestElement, P->ts);
        js_lex(P);
        r->a = js_parse_binding_atom(P);
        js_push(P, js_fin(P, r));
        rest = 1;
      } else js_push(P, js_parse_assign(P, 0));
      if (P->tt == TK_RPAREN) break;
      js_expect(P, TK_COMMA, "expected ',' in parenthesized expression");
      if (P->tt == TK_RPAREN) trailing = 1;
    }
    js_lex(P);
    size_t cnt = P->sp - mark;
    if (P->tt == TK_ARROW && !P->nl) {
      JsNode **params = NULL;
      uint32_t np = (uint32_t)cnt;
      if (cnt) {
        params = js_alloc(P, cnt * sizeof *params);
        for (size_t i = 0; i < cnt; i++) params[i] = js_to_pattern(P, P->stk[mark + i]);
      }
      P->sp = mark;
      return js_parse_arrow(P, start, params, np, 0);
    }
    if (rest || trailing || cnt == 0) js_fail(P, "invalid parenthesized expression");
    if (cnt == 1) { n = P->stk[mark]; P->sp = mark; return n; }
    JsNode **items = NULL;
    uint32_t nitems = 0;
    js_take_list(P, mark, &items, &nitems);
    n = js_node(P, N_SequenceExpression, items[0]->start);
    n->list = items; n->nlist = nitems;
    return js_fin(P, n);
  }
  default:
    js_fail(P, "unexpected token in expression");
  }
  return NULL;
}

static JsNode *js_parse_lhs(JsContext *P, int allow_call) {
  JsNode *e = (P->tt == TK_NEW) ? js_parse_new(P) : js_parse_primary(P);
  return js_parse_subscripts(P, e, allow_call);
}

static JsNode *js_parse_unary(JsContext *P) {
  size_t start = P->ts;
  JsNode *n;
  switch (P->tt) {
  case TK_BANG: case TK_TILDE: case TK_PLUS: case TK_MINUS: case TK_TYPEOF: case TK_VOID: case TK_DELETE:
    n = js_node(P, N_UnaryExpression, start);
    n->op = (uint8_t)P->tt;
    js_lex(P);
    n->a = js_parse_unary(P);
    return js_fin(P, n);
  case TK_INC: case TK_DEC:
    n = js_node(P, N_UpdateExpression, start);
    n->op = (uint8_t)P->tt; n->flags |= JF_PREFIX;
    js_lex(P);
    n->a = js_parse_unary(P);
    return js_fin(P, n);
  default:
    if (P->in_async && js_name_is(P, "await")) {
      n = js_node(P, N_AwaitExpression, start);
      js_lex(P);
      n->a = js_parse_unary(P);
      return js_fin(P, n);
    }
  }
  JsNode *e = js_parse_lhs(P, 1);
  if ((P->tt == TK_INC || P->tt == TK_DEC) && !P->nl) {
    n = js_node(P, N_UpdateExpression, e->start);
    n->op = (uint8_t)P->tt; n->a = e;
    js_lex(P);
    return js_fin(P, n);
  }
  return e;
}

static int js_bin_prec(int tt, int noin) {
  switch (tt) {
  case TK_NULLISH: return 1;
  case TK_OR: return 2;
  case TK_AND: return 3;
  case TK_BAR: return 4;
  case TK_CARET: return 5;
  case TK_AMP: return 6;
  case TK_EQ: case TK_NE: case TK_SEQ: case TK_SNE: return 7;
  case TK_LT: case TK_GT: case TK_LE: case TK_GE: case TK_INSTANCEOF: return 8;
  case TK_IN: return noin ? 0 : 8;
  case TK_SHL: case TK_SAR: case TK_SHR: return 9;
  case TK_PLUS: case TK_MINUS: return 10;
  case TK_STAR: case TK_SLASH: case TK_PERCENT: return 11;
  case TK_STARSTAR: return 12;
  }
  return 0;
}

static JsNode *js_parse_binary(JsContext *P, int minp, int noin) {
  JsNode *left = js_parse_unary(P);
  for (;;) {
    int tt = P->tt, p = js_bin_prec(tt, noin);
    if (p == 0 || p <= minp) break;
    js_lex(P);
    JsNode *right = js_parse_binary(P, tt == TK_STARSTAR ? p - 1 : p, noin);
    JsNode *n = js_node(P, (tt == TK_NULLISH || tt == TK_OR || tt == TK_AND) ? N_LogicalExpression
                                                                          : N_BinaryExpression,
                        left->start);
    n->op = (uint8_t)tt; n->a = left; n->b = right;
    left = js_fin(P, n);
  }
  return left;
}

static JsNode *js_parse_conditional(JsContext *P, int noin) {
  JsNode *test = js_parse_binary(P, 0, noin);
  if (P->tt != TK_QUESTION) return test;
  js_lex(P);
  JsNode *n = js_node(P, N_ConditionalExpression, test->start);
  n->a = test;
  n->b = js_parse_assign(P, 0);
  js_expect(P, TK_COLON, "expected ':'");
  n->c = js_parse_assign(P, noin);
  return js_fin(P, n);
}

static JsNode *js_parse_assign(JsContext *P, int noin) {
  size_t start = P->ts;
  if (P->in_generator && js_name_is(P, "yield")) {
    JsNode *y = js_node(P, N_YieldExpression, start);
    js_lex(P);
    if (!P->nl && P->tt == TK_STAR) { y->flags |= JF_DELEGATE; js_lex(P); y->a = js_parse_assign(P, noin); }
    else if (!P->nl && P->tt != TK_RPAREN && P->tt != TK_RBRACK && P->tt != TK_RBRACE &&
             P->tt != TK_COMMA && P->tt != TK_COLON && P->tt != TK_SEMI && P->tt != TK_EOF)
      y->a = js_parse_assign(P, noin);
    return js_fin(P, y);
  }
  JsNode *left = js_parse_conditional(P, noin);
  if (js_is_assign(P->tt)) {
    int op = P->tt;
    if (op == TK_ASSIGN && (left->type == N_ObjectExpression || left->type == N_ArrayExpression))
      left = js_to_pattern(P, left);
    js_lex(P);
    JsNode *n = js_node(P, N_AssignmentExpression, start);
    n->op = (uint8_t)op; n->a = left;
    n->b = js_parse_assign(P, noin);
    return js_fin(P, n);
  }
  return left;
}

static JsNode *js_parse_expression(JsContext *P, int noin) {
  JsNode *e = js_parse_assign(P, noin);
  if (P->tt != TK_COMMA) return e;
  size_t mark = P->sp;
  js_push(P, e);
  while (P->tt == TK_COMMA) { js_lex(P); js_push(P, js_parse_assign(P, noin)); }
  JsNode *n = js_node(P, N_SequenceExpression, e->start);
  js_take_list(P, mark, &n->list, &n->nlist);
  return js_fin(P, n);
}

/* ---- statements ---- */
static JsNode *js_parse_function_decl(JsContext *P, size_t start, int async, int is_decl) {
  JsNode *n = js_node(P, is_decl ? N_FunctionDeclaration : N_FunctionExpression, start);
  if (async) n->flags |= JF_ASYNC;
  js_lex(P);                                  /* 'function' */
  if (P->tt == TK_STAR) { n->flags |= JF_GENERATOR; js_lex(P); }
  if (P->tt == TK_NAME) n->a = js_take_ident(P);
  js_parse_fn_rest(P, n, async, (n->flags & JF_GENERATOR) != 0);
  return js_fin(P, n);
}

static int js_let_starts_decl(JsContext *P) {
  int t = js_peek(P, NULL);
  return t == TK_NAME || t == TK_LBRACK || t == TK_LBRACE;
}

static JsNode *js_parse_import(JsContext *P, size_t start) {
  JsNode *n = js_node(P, N_ImportDeclaration, start);
  size_t mark = P->sp;
  js_lex(P);                                  /* 'import' */
  if (P->tt != TK_STRING) {
    if (P->tt == TK_NAME) {
      JsNode *d = js_node(P, N_ImportDefaultSpecifier, P->ts);
      d->a = js_take_ident(P);
      js_push(P, js_fin(P, d));
      if (P->tt == TK_COMMA) js_lex(P);
    }
    if (P->tt == TK_STAR) {
      JsNode *ns = js_node(P, N_ImportNamespaceSpecifier, P->ts);
      js_lex(P);
      if (!js_name_is(P, "as")) js_fail(P, "expected 'as'");
      js_lex(P);
      ns->a = js_take_ident(P);
      js_push(P, js_fin(P, ns));
    } else if (P->tt == TK_LBRACE) {
      js_lex(P);
      while (P->tt != TK_RBRACE) {
        JsNode *s = js_node(P, N_ImportSpecifier, P->ts);
        s->a = js_take_word(P);
        if (js_name_is(P, "as")) { js_lex(P); s->b = js_take_ident(P); }
        else s->b = js_clone_ident(P, s->a);
        js_push(P, js_fin(P, s));
        if (P->tt != TK_RBRACE) js_expect(P, TK_COMMA, "expected ',' in import list");
      }
      js_lex(P);
    }
    if (!js_name_is(P, "from")) js_fail(P, "expected 'from'");
    js_lex(P);
  }
  if (P->tt != TK_STRING) js_fail(P, "expected module specifier");
  n->a = js_take_literal(P, N_StringLiteral);
  js_consume_semi(P);
  js_take_list(P, mark, &n->list, &n->nlist);
  return js_fin(P, n);
}

static JsNode *js_parse_export(JsContext *P, size_t start) {
  js_lex(P);                                  /* 'export' */
  if (P->tt == TK_STAR) {
    JsNode *n = js_node(P, N_ExportAllDeclaration, start);
    js_lex(P);
    if (js_name_is(P, "as")) js_fail(P, "export * as ns is not supported by this parser");
    if (!js_name_is(P, "from")) js_fail(P, "expected 'from'");
    js_lex(P);
    n->a = js_take_literal(P, N_StringLiteral);
    js_consume_semi(P);
    return js_fin(P, n);
  }
  if (P->tt == TK_DEFAULT) {
    JsNode *n = js_node(P, N_ExportDefaultDeclaration, start);
    size_t s = P->ts;
    js_lex(P);
    int t = P->tt, nl = 0;
    if (t == TK_FUNCTION) n->a = js_parse_function_decl(P, s, 0, 1);
    else if (t == TK_CLASS) n->a = js_parse_class(P, s, 1);
    else if (js_name_is(P, "async") && js_peek(P, &nl) == TK_FUNCTION && !nl) {
      js_lex(P);
      n->a = js_parse_function_decl(P, s, 1, 1);
    } else {
      n->a = js_parse_assign(P, 0);
      js_consume_semi(P);
    }
    return js_fin(P, n);
  }
  if (P->tt == TK_LBRACE) {
    JsNode *n = js_node(P, N_ExportNamedDeclaration, start);
    size_t mark = P->sp;
    js_lex(P);
    while (P->tt != TK_RBRACE) {
      JsNode *s = js_node(P, N_ExportSpecifier, P->ts);
      s->a = js_take_word(P);
      if (js_name_is(P, "as")) { js_lex(P); s->b = js_take_word(P); }
      else s->b = js_clone_ident(P, s->a);
      js_push(P, js_fin(P, s));
      if (P->tt != TK_RBRACE) js_expect(P, TK_COMMA, "expected ',' in export list");
    }
    js_lex(P);
    if (js_name_is(P, "from")) { js_lex(P); n->b = js_take_literal(P, N_StringLiteral); }
    js_consume_semi(P);
    js_take_list(P, mark, &n->list, &n->nlist);
    return js_fin(P, n);
  }
  JsNode *n = js_node(P, N_ExportNamedDeclaration, start);
  if (!(P->tt == TK_VAR || P->tt == TK_CONST || P->tt == TK_FUNCTION || P->tt == TK_CLASS ||
        js_name_is(P, "let") || js_name_is(P, "async")))
    js_fail(P, "unsupported export form");
  n->a = js_parse_statement_list_item(P);
  return js_fin(P, n);
}

static JsNode *js_parse_statement_list_item(JsContext *P) {
  size_t start = P->ts;
  int nl = 0;
  switch (P->tt) {
  case TK_FUNCTION: return js_parse_function_decl(P, start, 0, 1);
  case TK_CLASS: return js_parse_class(P, start, 1);
  case TK_CONST: {
    js_lex(P);
    JsNode *n = js_parse_var(P, start, 2, 0);
    js_consume_semi(P);
    return js_fin(P, n);
  }
  case TK_NAME:
    if (js_name_is(P, "let") && js_let_starts_decl(P)) {
      js_lex(P);
      JsNode *n = js_parse_var(P, start, 1, 0);
      js_consume_semi(P);
      return js_fin(P, n);
    }
    if (js_name_is(P, "async") && js_peek(P, &nl) == TK_FUNCTION && !nl) {
      js_lex(P);
      return js_parse_function_decl(P, start, 1, 1);
    }
    break;
  case TK_IMPORT: {
    int t = js_peek(P, NULL);
    if (t != TK_LPAREN && t != TK_DOT) {
      if (!P->module) js_fail(P, "import declaration outside a module");
      return js_parse_import(P, start);
    }
    break;
  }
  case TK_EXPORT:
    if (!P->module) js_fail(P, "export declaration outside a module");
    return js_parse_export(P, start);
  default: break;
  }
  return js_parse_statement(P);
}

static JsNode *js_parse_statement(JsContext *P) {
  size_t start = P->ts;
  JsNode *n;
  switch (P->tt) {
  case TK_LBRACE: return js_parse_block(P, 0);
  case TK_SEMI: n = js_node(P, N_EmptyStatement, start); js_lex(P); return js_fin(P, n);
  case TK_VAR: {
    js_lex(P);
    n = js_parse_var(P, start, 0, 0);
    js_consume_semi(P);
    return js_fin(P, n);
  }
  case TK_IF:
    n = js_node(P, N_IfStatement, start);
    js_lex(P);
    js_expect(P, TK_LPAREN, "expected '('");
    n->a = js_parse_expression(P, 0);
    js_expect(P, TK_RPAREN, "expected ')'");
    n->b = js_parse_statement(P);
    if (P->tt == TK_ELSE) { js_lex(P); n->c = js_parse_statement(P); }
    return js_fin(P, n);
  case TK_WHILE:
    n = js_node(P, N_WhileStatement, start);
    js_lex(P);
    js_expect(P, TK_LPAREN, "expected '('");
    n->a = js_parse_expression(P, 0);
    js_expect(P, TK_RPAREN, "expected ')'");
    n->b = js_parse_statement(P);
    return js_fin(P, n);
  case TK_DO:
    n = js_node(P, N_DoWhileStatement, start);
    js_lex(P);
    n->b = js_parse_statement(P);
    js_expect(P, TK_WHILE, "expected 'while'");
    js_expect(P, TK_LPAREN, "expected '('");
    n->a = js_parse_expression(P, 0);
    js_expect(P, TK_RPAREN, "expected ')'");
    if (P->tt == TK_SEMI) js_lex(P);
    return js_fin(P, n);
  case TK_FOR: {
    js_lex(P);
    int is_await = 0;
    if (js_name_is(P, "await")) { is_await = 1; js_lex(P); }
    js_expect(P, TK_LPAREN, "expected '('");
    JsNode *init = NULL;
    if (P->tt == TK_SEMI) {
      /* no initializer */
    } else if (P->tt == TK_VAR || P->tt == TK_CONST || (js_name_is(P, "let") && js_let_starts_decl(P))) {
      size_t ds = P->ts;
      int kind = P->tt == TK_VAR ? 0 : P->tt == TK_CONST ? 2 : 1;
      js_lex(P);
      init = js_parse_var(P, ds, kind, 1);
      js_fin(P, init);
    } else {
      init = js_parse_expression(P, 1);
    }
    if (init && (P->tt == TK_IN || js_name_is(P, "of"))) {
      int of = P->tt != TK_IN;
      js_lex(P);
      if (init->type != N_VariableDeclaration) init = js_to_pattern(P, init);
      n = js_node(P, of ? N_ForOfStatement : N_ForInStatement, start);
      if (is_await) n->flags |= JF_ASYNC;
      n->a = init;
      n->b = of ? js_parse_assign(P, 0) : js_parse_expression(P, 0);
      js_expect(P, TK_RPAREN, "expected ')'");
      n->c = js_parse_statement(P);
      return js_fin(P, n);
    }
    n = js_node(P, N_ForStatement, start);
    n->a = init;
    js_expect(P, TK_SEMI, "expected ';'");
    if (P->tt != TK_SEMI) n->b = js_parse_expression(P, 0);
    js_expect(P, TK_SEMI, "expected ';'");
    if (P->tt != TK_RPAREN) n->c = js_parse_expression(P, 0);
    js_expect(P, TK_RPAREN, "expected ')'");
    n->d = js_parse_statement(P);
    return js_fin(P, n);
  }
  case TK_RETURN:
    n = js_node(P, N_ReturnStatement, start);
    js_lex(P);
    if (P->tt != TK_SEMI && P->tt != TK_RBRACE && P->tt != TK_EOF && !P->nl) n->a = js_parse_expression(P, 0);
    js_consume_semi(P);
    return js_fin(P, n);
  case TK_THROW:
    n = js_node(P, N_ThrowStatement, start);
    js_lex(P);
    if (P->nl) js_fail(P, "newline after throw");
    n->a = js_parse_expression(P, 0);
    js_consume_semi(P);
    return js_fin(P, n);
  case TK_BREAK: case TK_CONTINUE:
    n = js_node(P, P->tt == TK_BREAK ? N_BreakStatement : N_ContinueStatement, start);
    js_lex(P);
    if (P->tt == TK_NAME && !P->nl) n->a = js_take_ident(P);
    js_consume_semi(P);
    return js_fin(P, n);
  case TK_TRY: {
    n = js_node(P, N_TryStatement, start);
    js_lex(P);
    n->a = js_parse_block(P, 0);
    if (P->tt == TK_CATCH) {
      JsNode *c = js_node(P, N_CatchClause, P->ts);
      js_lex(P);
      if (P->tt == TK_LPAREN) {
        js_lex(P);
        c->a = js_parse_binding_atom(P);
        js_expect(P, TK_RPAREN, "expected ')'");
      }
      c->b = js_parse_block(P, 0);
      n->b = js_fin(P, c);
    }
    if (P->tt == TK_FINALLY) { js_lex(P); n->c = js_parse_block(P, 0); }
    if (!n->b && !n->c) js_fail(P, "try without catch or finally");
    return js_fin(P, n);
  }
  case TK_SWITCH: {
    n = js_node(P, N_SwitchStatement, start);
    js_lex(P);
    js_expect(P, TK_LPAREN, "expected '('");
    n->a = js_parse_expression(P, 0);
    js_expect(P, TK_RPAREN, "expected ')'");
    js_expect(P, TK_LBRACE, "expected '{'");
    size_t mark = P->sp;
    while (P->tt != TK_RBRACE) {
      JsNode *c = js_node(P, N_SwitchCase, P->ts);
      if (P->tt == TK_CASE) { js_lex(P); c->a = js_parse_expression(P, 0); }
      else if (P->tt == TK_DEFAULT) js_lex(P);
      else js_fail(P, "expected case or default");
      js_expect(P, TK_COLON, "expected ':'");
      size_t cm = P->sp;
      while (P->tt != TK_CASE && P->tt != TK_DEFAULT && P->tt != TK_RBRACE) {
        if (P->tt == TK_EOF) js_fail(P, "unexpected end of input");
        js_push(P, js_parse_statement_list_item(P));
      }
      js_take_list(P, cm, &c->list, &c->nlist);
      js_push(P, js_fin(P, c));
    }
    js_lex(P);
    js_take_list(P, mark, &n->list, &n->nlist);
    return js_fin(P, n);
  }
  case TK_WITH:
    n = js_node(P, N_WithStatement, start);
    js_lex(P);
    js_expect(P, TK_LPAREN, "expected '('");
    n->a = js_parse_expression(P, 0);
    js_expect(P, TK_RPAREN, "expected ')'");
    n->b = js_parse_statement(P);
    return js_fin(P, n);
  case TK_DEBUGGER:
    n = js_node(P, N_DebuggerStatement, start);
    js_lex(P);
    js_consume_semi(P);
    return js_fin(P, n);
  case TK_FUNCTION: case TK_CLASS: case TK_CONST:
    return js_parse_statement_list_item(P);
  default: break;
  }
  if (P->tt == TK_NAME) {
    int t = js_peek(P, NULL);
    if (t == TK_COLON) {
      n = js_node(P, N_LabeledStatement, start);
      n->a = js_take_ident(P);
      js_lex(P);
      n->b = js_parse_statement(P);
      return js_fin(P, n);
    }
  }
  n = js_node(P, N_ExpressionStatement, start);
  n->a = js_parse_expression(P, 0);
  js_consume_semi(P);
  return js_fin(P, n);
}

/* ---- entry points ---- */
/* Parses src and returns the File node (File.a is the Program). The arena of ctx is reused, so
 * nodes from an earlier js_parse on the same context are invalid after this call. *tokens_out
 * receives the Babel-style token count. */
static JsNode *js_parse(JsContext *ctx, const char *src, size_t len, int module, size_t *tokens_out) {
  js_arena_reset(&ctx->ar);
  ctx->sp = 0;
  if (!js_cls_ready) js_init_classes();
  ctx->src = src; ctx->len = len;
  ctx->pos = 0; ctx->ts = 0; ctx->te = 0; ctx->prev_end = 0; ctx->ntok = 0;
  ctx->tt = TK_EOF; ctx->nl = 0; ctx->module = module;
  ctx->in_function = 0; ctx->in_async = 0; ctx->in_generator = 0;
  JsNode *file = js_node(ctx, N_File, 0);
  JsNode *prog = js_node(ctx, N_Program, 0);
  js_lex(ctx);
  js_parse_stmt_list(ctx, TK_EOF, prog, 1);
  prog->end = (uint32_t)len;
  file->a = prog;
  file->end = (uint32_t)len;
  if (tokens_out) *tokens_out = ctx->ntok;
  return file;
}

/* Counts the nodes reachable from n (Babel-style node count; comments and tokens excluded). */
static size_t js_count_nodes(const JsNode *n) {
  if (!n) return 0;
  size_t c = 1;
  c += js_count_nodes(n->a) + js_count_nodes(n->b) + js_count_nodes(n->c) + js_count_nodes(n->d);
  for (uint32_t i = 0; i < n->nlist; i++) c += js_count_nodes(n->list[i]);
  for (uint32_t i = 0; i < n->nlist2; i++) c += js_count_nodes(n->list2[i]);
  return c;
}

/* ---- files and timing shared by the rows ---- */
static char *js_read_file(const char *path, size_t *len_out) {
  FILE *f = fopen(path, "rb");
  if (!f) { fprintf(stderr, "cannot open %s\n", path); exit(2); }
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  char *buf = malloc((size_t)n + 1);
  if (!buf || fread(buf, 1, (size_t)n, f) != (size_t)n) { fprintf(stderr, "cannot read %s\n", path); exit(2); }
  fclose(f);
  buf[n] = 0;
  *len_out = (size_t)n;
  return buf;
}
static double js_now_ms(void) {
  struct timespec ts;
  timespec_get(&ts, TIME_UTC);
  return ts.tv_sec * 1e3 + ts.tv_nsec / 1e6;
}

#endif /* JS_PARSER_ANALOGUE_H */
