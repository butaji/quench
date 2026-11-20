/* WSL parser: a C port of WSL/Parse.js (parse(program, origin, originKind, lineNumberOffset, text)).
 *
 * Structure follows Parse.js closure by closure. Syntax errors (Lexer.fail, WSyntaxError) unwind
 * with longjmp to the innermost handler; a backtracking scope catches only WSyntaxError and
 * restores the lexer state, as Lexer.backtrackingScope does. Other errors (WTypeError from
 * Program.add, and the JS engine errors Parse.js can throw) unwind to the top-level handler.
 *
 * Quirks kept on purpose, because the row's results depend on them:
 *   - genericParseCommaExpression discards the effectful prefix (its backtracking callback
 *     returns undefined, so the loop always takes the final expression);
 *   - the post-increment suffix accepts only "." (parseSuffixOperator receives a string);
 *   - consumeEndOfTypeArgs pushes a token whose text is "punctuation" (the JS LexerToken
 *     arguments are in the wrong order), which then fails the next consume;
 *   - "?" reaches the ternary branch, which names an undefined class (ReferenceError).
 *
 * Program.add checks are done in program_add: the row's NameContext duplicate-name rules for
 * top-level things. The native-intrinsic check (NameContext.add, native funcs without an
 * implementation) is not reproduced: it only runs for the standard library, which the row
 * parses without error. */
#ifndef WSL_PARSE_H
#define WSL_PARSE_H

#include <setjmp.h>
#include "wsl-lex.h"
#include "wsl-ast.h"

/* ---- errors ---- */
static jmp_buf *WSL_HANDLERS[64];
static int WSL_NH;
static char WSL_ERR_CLASS[32];
static char WSL_ERR_MSG[16384];   /* a type error can list every overload that failed */
static int WSL_ERR_SYNTAX;

/* Raises an error. Syntax errors can be caught by backtracking scopes; the rest cannot. */
static void wsl_throw(int syntax, const char *cls, const char *msg) {
  snprintf(WSL_ERR_CLASS, sizeof WSL_ERR_CLASS, "%s", cls);
  snprintf(WSL_ERR_MSG, sizeof WSL_ERR_MSG, "%s", msg);
  WSL_ERR_SYNTAX = syntax;
  if (WSL_NH == 0) { fprintf(stderr, "unhandled parse error: %s: %s\n", cls, msg); exit(5); }
  longjmp(*WSL_HANDLERS[WSL_NH - 1], 1);
}

/* ---- parser state ---- */
typedef struct {
  Lexer L;
  int nstack;
  Tok stack[16];
} Parser;

typedef struct { int index; int nstack; Tok stack[16]; } LexState;

static LexState save_state(const Parser *P) {
  LexState s;
  s.index = P->L.index;
  s.nstack = P->nstack;
  memcpy(s.stack, P->stack, sizeof(Tok) * P->nstack);
  return s;
}
static void restore_state(Parser *P, const LexState *s) {
  P->L.index = s->index;
  P->nstack = s->nstack;
  memcpy(P->stack, s->stack, sizeof(Tok) * s->nstack);
}

static int tok_present(Tok t) { return t.text != NULL; }
static int tok_eq(Tok t, const char *s) {
  size_t n = strlen(s);
  return t.text && (size_t)t.len == n && memcmp(t.text, s, n) == 0;
}
static int tok_in(Tok t, const char *const *texts) {
  for (int i = 0; texts[i]; i++) if (tok_eq(t, texts[i])) return 1;
  return 0;
}
static int tok_kind_is(Tok t, const char *kind) {
  return t.kind && strcmp(t.kind, kind) == 0;
}

/* Lexer.fail: a WSyntaxError at the lexer's current position. Inside a backtracking scope the
 * error is always caught by the innermost scope, so its text is never read and is not built
 * (msg may then be NULL; callers pass a message only at the top level). */
static void p_fail(Parser *P, const char *msg) {
  if (WSL_NH > 1) {
    WSL_ERR_SYNTAX = 1;
    longjmp(*WSL_HANDLERS[WSL_NH - 1], 1);
  }
  char buf[512];
  snprintf(buf, sizeof buf, "Syntax error at %s:%d: %s", P->L.origin, lex_line_of(&P->L, P->L.index), msg);
  wsl_throw(1, "WSyntaxError", buf);
}
#define P_FAIL_FMT(P, ...) do { if (WSL_NH > 1) p_fail(P, NULL); else { char m_[512]; snprintf(m_, sizeof m_, __VA_ARGS__); p_fail(P, m_); } } while (0)

/* Lexer.next(): pops the pushed tokens first; a null token is end of input. */
static Tok P_next(Parser *P) {
  if (P->nstack) return P->stack[--P->nstack];
  Tok t = {NULL, NULL, 0, 0};
  if (P->L.index >= P->L.len) return t;
  int r = lex_next(&P->L, &t);
  if (r < 0) p_fail(P, P->L.errmsg);
  if (r == 0) { t.kind = NULL; t.text = NULL; }
  return t;
}
static void P_push(Parser *P, Tok t) { P->stack[P->nstack++] = t; }
static Tok P_peek(Parser *P) {
  Tok t = P_next(P);
  P_push(P, t);
  return t;
}

static char *join_texts(const char *const *texts, char *buf, size_t cap) {
  buf[0] = 0;
  for (int i = 0; texts[i]; i++) {
    if (i) strncat(buf, ",", cap - strlen(buf) - 1);
    strncat(buf, texts[i], cap - strlen(buf) - 1);
  }
  return buf;
}

/* genericConsume */
static Tok consume_texts(Parser *P, const char *const *texts) {
  Tok t = P_next(P);
  if (!tok_present(t)) p_fail(P, "Unexpected end of file");
  if (!tok_in(t, texts)) {
    if (WSL_NH > 1) p_fail(P, NULL);
    char expl[256];
    join_texts(texts, expl, sizeof expl);
    P_FAIL_FMT(P, "Unexpected token: %.*s; expected: %s", t.len, t.text, expl);
  }
  return t;
}
static Tok consume_kind(Parser *P, const char *kind) {
  Tok t = P_next(P);
  if (!tok_present(t)) p_fail(P, "Unexpected end of file");
  if (!tok_kind_is(t, kind)) {
    P_FAIL_FMT(P, "Unexpected token: %.*s; expected: %s", t.len, t.text, kind);
  }
  return t;
}
static Tok test_texts(Parser *P, const char *const *texts) {
  Tok t = P_peek(P);
  Tok none = {NULL, NULL, 0, 0};
  return tok_present(t) && tok_in(t, texts) ? t : none;
}
static Tok test_kind(Parser *P, const char *kind) {
  Tok t = P_peek(P);
  Tok none = {NULL, NULL, 0, 0};
  return tok_present(t) && tok_kind_is(t, kind) ? t : none;
}
static Tok try_consume_texts(Parser *P, const char *const *texts) {
  Tok t = test_texts(P, texts);
  if (tok_present(t)) P_next(P);
  return t;
}
static Tok try_consume_kind(Parser *P, const char *kind) {
  Tok t = test_kind(P, kind);
  if (tok_present(t)) P_next(P);
  return t;
}
static void assert_next(Parser *P, const char *const *texts) {
  P_push(P, consume_texts(P, texts));
}

#define TXT(...) ((const char *const[]){__VA_ARGS__, NULL})
#define CONSUME(P, ...) consume_texts(P, TXT(__VA_ARGS__))
#define TEST(P, ...) test_texts(P, TXT(__VA_ARGS__))
#define TRY(P, ...) try_consume_texts(P, TXT(__VA_ARGS__))
#define ASSERT_NEXT(P, ...) assert_next(P, TXT(__VA_ARGS__))

static const char *const ADDRESS_SPACE_TEXTS[] = {"constant", "device", "threadgroup", "thread", NULL};

/* ---- backtracking (Lexer.backtrackingScope, Lexer.testScope) ---- */
typedef Ast *(*ScopeFn)(Parser *P, void *ctx);

static void rethrow_outer(void) {
  if (WSL_NH == 0) { fprintf(stderr, "unhandled parse error: %s: %s\n", WSL_ERR_CLASS, WSL_ERR_MSG); exit(5); }
  longjmp(*WSL_HANDLERS[WSL_NH - 1], 1);
}

/* Returns the callback's result. On WSyntaxError the state is restored and NULL returned.
 * The JS callbacks used here return undefined on success, so the caller treats the result as a
 * node or NULL exactly as the JS does. */
static Ast *backtracking_scope(Parser *P, ScopeFn fn, void *ctx) {
  LexState st = save_state(P);
  jmp_buf jb;
  int depth = WSL_NH;
  WSL_HANDLERS[WSL_NH++] = &jb;
  if (setjmp(jb) == 0) {
    Ast *r = fn(P, ctx);
    WSL_NH = depth;
    return r;
  }
  WSL_NH = depth;
  if (!WSL_ERR_SYNTAX) rethrow_outer();
  restore_state(P, &st);
  return NULL;
}

typedef void (*ScopeVoidFn)(Parser *P);
/* Lexer.testScope: true when fn parses; the state is always restored. */
static int test_scope(Parser *P, ScopeVoidFn fn) {
  LexState st = save_state(P);
  jmp_buf jb;
  int depth = WSL_NH;
  int ok;
  WSL_HANDLERS[WSL_NH++] = &jb;
  if (setjmp(jb) == 0) {
    fn(P);
    WSL_NH = depth;
    ok = 1;
  } else {
    WSL_NH = depth;
    if (!WSL_ERR_SYNTAX) rethrow_outer();
    ok = 0;
  }
  restore_state(P, &st);
  return ok;
}

/* ---- Program.add (top-level name rules of NameContext.add) ---- */
typedef struct { const char *name; int isFunc; } NameEnt;
#define NAME_TAB_SIZE 4096
typedef struct {
  AstList statements;
  NameEnt tab[NAME_TAB_SIZE];
  char used[NAME_TAB_SIZE];
} Program;

static unsigned name_hash(const char *s) {
  unsigned h = 2166136261u;
  for (; *s; s++) { h ^= (unsigned char)*s; h *= 16777619u; }
  return h;
}
static int name_lookup(Program *prog, const char *name, int *found_func) {
  unsigned i = name_hash(name) & (NAME_TAB_SIZE - 1);
  for (;;) {
    if (!prog->used[i]) return -1;
    if (strcmp(prog->tab[i].name, name) == 0) { *found_func = prog->tab[i].isFunc; return (int)i; }
    i = (i + 1) & (NAME_TAB_SIZE - 1);
  }
}
static void name_insert(Program *prog, const char *name, int isFunc) {
  unsigned i = name_hash(name) & (NAME_TAB_SIZE - 1);
  while (prog->used[i]) i = (i + 1) & (NAME_TAB_SIZE - 1);
  prog->used[i] = 1;
  prog->tab[i].name = name;
  prog->tab[i].isFunc = isFunc;
}

/* The row's WTypeError messages carry the origin token's position. */
static void type_error_at(Parser *P, int origin, const char *msg) {
  char buf[512];
  snprintf(buf, sizeof buf, "Type error at %s:%d: %s", P->L.origin, lex_line_of(&P->L, origin), msg);
  wsl_throw(0, "WTypeError", buf);
}

static void program_add(Parser *P, Program *prog, Ast *st) {
  int isFunc = st->tag == A_FUNCDEF || st->tag == A_NATIVEFUNC;
  const char *name;
  switch (st->tag) {
  case A_FUNCDEF: case A_NATIVEFUNC: name = st->u.func.name; break;
  case A_TYPEDEF: name = st->u.typeDef.name; break;
  case A_STRUCTTYPE: name = st->u.structType.name; break;
  case A_ENUMTYPE: name = st->u.enumType.name; break;
  case A_NATIVETYPE: name = st->u.nativeType.name; break;
  case A_PROTOCOLDECL: name = st->u.protocolDecl.name; break;
  default: name = NULL;
  }
  if (name) {
    int existing_func = 0;
    int slot = name_lookup(prog, name, &existing_func);
    if (isFunc) {
      if (slot >= 0 && !existing_func) {
        char msg[300];
        snprintf(msg, sizeof msg, "Cannot reuse type name for function: %s", name);
        type_error_at(P, st->origin, msg);
      }
      if (slot < 0) name_insert(prog, name, 1);
    } else {
      if (slot >= 0) {
        char msg[300];
        snprintf(msg, sizeof msg, "Duplicate name: %s", name);
        type_error_at(P, st->origin, msg);
      }
      name_insert(prog, name, 0);
    }
  }
  list_push(&prog->statements, st);
}

/* ---- the parser (Parse.js closures) ---- */
static Ast *parse_expression(Parser *P);
static Ast *parse_possible_assignment(Parser *P, int required);
static Ast *parse_possible_prefix(Parser *P);
static Ast *parse_possible_logical_or(Parser *P);
static Ast *parse_type(Parser *P);
static Ast *parse_statement(Parser *P);
static Ast *parse_block(Parser *P);
static Ast *parse_effectful_expression(Parser *P);
static void parse_type_parameters(Parser *P, AstList *out);
static void parse_type_arguments(Parser *P, AstList *out);
static Ast *parse_variable_decls(Parser *P);

static Ast *parse_term(Parser *P);
static Ast *parse_constexpr(Parser *P);
static Ast *parse_effectful_expression(Parser *P);
static Ast *parse_effectful_statement(Parser *P);
static Ast *parse_block(Parser *P);
static Ast *parse_func_def(Parser *P);
static Ast *parse_variable_decls(Parser *P);

static Ast *parse_protocol_ref(Parser *P) {
  Tok t = consume_kind(P, "identifier");
  Ast *a = ast_new(A_PROTOCOLREF, t.index);
  a->u.nameOnly.name = arena_strndup(t.text, t.len);
  return a;
}

static void consume_end_of_type_args(Parser *P) {
  Tok rightShift = TRY(P, ">>");
  if (tok_present(rightShift)) {
    /* new LexerToken(lexer, rightShift, rightShift.index, rightShift.kind, ">") in Parse.js: the
     * constructor receives the wrong arguments, so the pushed token reads "punctuation". */
    Tok pushed;
    pushed.kind = "#number";
    pushed.text = "punctuation";
    pushed.len = 11;
    pushed.index = rightShift.index;
    P_push(P, pushed);
  } else
    CONSUME(P, ">");
}

/* parseTypeParameters, in the backtracking callback form */
static Ast *cb_constexpr_type_param(Parser *P, void *ctx) {
  (void)ctx;
  Ast *type = parse_type(P);
  Tok name = consume_kind(P, "identifier");
  ASSERT_NEXT(P, ",", ">", ">>");
  Ast *a = ast_new(A_CONSTEXPRTYPEPARAM, type->origin);
  a->u.named.name = arena_strndup(name.text, name.len);
  a->u.named.type = type;
  return a;
}

static void parse_type_parameters(Parser *P, AstList *out) {
  if (!tok_present(TEST(P, "<"))) return;
  CONSUME(P, "<");
  while (!tok_present(TEST(P, ">"))) {
    Ast *constexpr = backtracking_scope(P, cb_constexpr_type_param, NULL);
    if (constexpr)
      list_push(out, constexpr);
    else {
      Tok name = consume_kind(P, "identifier");
      Ast *protocol = tok_present(TRY(P, ":")) ? parse_protocol_ref(P) : NULL;
      Ast *a = ast_new(A_TYPEVARIABLE, name.index);
      a->u.typeVariable.name = arena_strndup(name.text, name.len);
      a->u.typeVariable.protocol = protocol;
      list_push(out, a);
    }
    if (!tok_present(TRY(P, ",")))
      break;
  }
  consume_end_of_type_args(P);
}

static Ast *cb_type_or_variable_ref(Parser *P, void *ctx) {
  (void)ctx;
  Tok result = consume_kind(P, "identifier");
  ASSERT_NEXT(P, ",", ">", ">>");
  Ast *a = ast_new(A_TYPEORVARIABLEREF, result.index);
  a->u.nameOnly.name = arena_strndup(result.text, result.len);
  return a;
}
static Ast *cb_constexpr_arg(Parser *P, void *ctx) {
  (void)ctx;
  Ast *result = parse_constexpr(P);
  ASSERT_NEXT(P, ",", ">", ">>");
  return result;
}

static void parse_type_arguments(Parser *P, AstList *out) {
  if (!tok_present(TEST(P, "<"))) return;
  CONSUME(P, "<");
  while (!tok_present(TEST(P, ">"))) {
    Ast *typeOrVariableRef = backtracking_scope(P, cb_type_or_variable_ref, NULL);
    if (typeOrVariableRef)
      list_push(out, typeOrVariableRef);
    else {
      Ast *constexpr = backtracking_scope(P, cb_constexpr_arg, NULL);
      if (constexpr)
        list_push(out, constexpr);
      else
        list_push(out, parse_type(P));
    }
    if (!tok_present(TRY(P, ",")))
      break;
  }
  consume_end_of_type_args(P);
}

static Ast *parse_term(Parser *P) {
  Tok t;
  if (tok_present(t = TRY(P, "null"))) {
    return lit_expr_new(A_NULLLITERAL, t.index);
  }
  if (tok_present(t = try_consume_kind(P, "identifier"))) {
    Ast *a = ast_new(A_VARIABLEREF, t.index);
    a->u.varRef.name = arena_strndup(t.text, t.len);
    return a;
  }
  if (tok_present(t = try_consume_kind(P, "intLiteral"))) {
    /* (+text) | 0 must print back as text: canonical digits and at most INT32_MAX */
    int canonical = !(t.len > 1 && t.text[0] == '0');
    long long v = 0;
    for (int i = 0; i < t.len; i++) { v = v * 10 + (t.text[i] - '0'); if (v > 0x7fffffffLL * 10) v = 0x7fffffffLL * 10; }
    if (!canonical || v > 0x7fffffffLL) P_FAIL_FMT(P, "Integer literal is not an integer: %.*s", t.len, t.text);
    Ast *a = lit_expr_new(A_INTLITERAL, t.index);
    a->u.intLit.value = v;
    return a;
  }
  if (tok_present(t = try_consume_kind(P, "uintLiteral"))) {
    int digits = t.len - 1;
    int canonical = !(digits > 1 && t.text[0] == '0');
    long long v = 0;
    for (int i = 0; i < digits; i++) { v = v * 10 + (t.text[i] - '0'); if (v > 0xffffffffLL * 10) v = 0xffffffffLL * 10; }
    if (!canonical || v > 0xffffffffLL) P_FAIL_FMT(P, "Integer literal is not 32-bit unsigned integer: %.*s", t.len, t.text);
    Ast *a = lit_expr_new(A_UINTLITERAL, t.index);
    a->u.intLit.value = v;
    return a;
  }
  {
    Tok h = try_consume_kind(P, "intHexLiteral");
    int isInt = tok_present(h);
    if (!isInt) h = try_consume_kind(P, "uintHexLiteral");
    if (tok_present(h)) {
      int hl = h.len - 2;
      const char *hs = h.text + 2;
      if (!isInt) hl--;   /* uintHexLiteral ends with 'u' */
      int canonical = hl > 0 && !(hl > 1 && hs[0] == '0');
      unsigned long long v = 0;
      for (int i = 0; i < hl && canonical; i++) {
        if (hs[i] >= 'A' && hs[i] <= 'F') canonical = 0;   /* toString(16) is lowercase */
        int d = lex_is_digit((unsigned char)hs[i]) ? hs[i] - '0' : (hs[i] | 0x20) - 'a' + 10;
        if (hl > 8) { canonical = 0; break; }
        v = v * 16 + (unsigned)d;
      }
      unsigned long long limit = isInt ? 0x7fffffffULL : 0xffffffffULL;
      if (!canonical || v > limit) P_FAIL_FMT(P, "Hex integer literal is not an integer: %.*s", h.len, h.text);
      Ast *a = lit_expr_new(isInt ? A_INTLITERAL : A_UINTLITERAL, h.index);
      a->u.intLit.value = (long long)v;
      return a;
    }
  }
  if (tok_present(t = try_consume_kind(P, "floatLiteral"))) {
    int d = t.len > 0 && t.text[t.len - 1] == 'd';
    int f = t.len > 0 && t.text[t.len - 1] == 'f';
    int n = t.len;
    if (d || f) n--;
    char *text = arena_strndup(t.text, n);
    double value = strtod(text, NULL);
    if (d) {
      Ast *a = lit_expr_new(A_DOUBLELITERAL, t.index);
      a->u.fltLit.value = value;
      return a;
    }
    Ast *a = lit_expr_new(A_FLOATLITERAL, t.index);
    a->u.fltLit.value = (double)(float)value;
    return a;
  }
  if (tok_present(t = TRY(P, "true", "false"))) {
    Ast *a = ast_new(A_BOOLLITERAL, t.index);
    a->u.boolLit.value = tok_eq(t, "true");
    return a;
  }
  CONSUME(P, "(");
  Ast *result = parse_expression(P);
  CONSUME(P, ")");
  return result;
}

static Ast *parse_constexpr(Parser *P);

/* ---- node builders (Parse.js constructor calls) ---- */
static Ast *mk_call(int origin, const char *name) {
  Ast *a = ast_new(A_CALL, origin);
  a->u.call.name = name;
  return a;
}
static Ast *mk_bool_call(Ast *arg) {
  Ast *c = mk_call(arg->origin, "bool");
  list_push(&c->u.call.argumentList, arg);
  return c;
}
static Ast *mk_dot(int origin, Ast *base, const char *field) {
  Ast *a = ast_new(A_DOT, origin);
  a->u.dot.base = base;
  a->u.dot.fieldName = field;
  return a;
}
/* A variable reference; an anon ref (1 old, 2 new value of a read-modify-write) names the
 * read-modify-write's own anonymous variable, which the caller passes. */
static Ast *mk_var_ref(int origin, const char *name, int anon, Ast *var) {
  Ast *a = ast_new(A_VARIABLEREF, origin);
  a->u.varRef.name = name;
  a->u.varRef.anon = anon;
  a->u.varRef.variable = var;
  return a;
}
static const char *tok_string(Tok t) { return arena_strndup(t.text, t.len); }
static const char *concat2(const char *a, const char *b) {
  size_t na = strlen(a), nb = strlen(b);
  char *s = arena_alloc(na + nb + 1);
  memcpy(s, a, na);
  memcpy(s + na, b, nb + 1);
  return s;
}

/* ---- types ---- */
static Ast *parse_type_def(Parser *P) {
  Tok origin = CONSUME(P, "typedef");
  Tok name = consume_kind(P, "identifier");
  Ast *a = ast_new(A_TYPEDEF, origin.index);
  a->u.typeDef.name = tok_string(name);
  parse_type_parameters(P, &a->u.typeDef.typeParameters);
  CONSUME(P, "=");
  a->u.typeDef.type = parse_type(P);
  CONSUME(P, ";");
  return a;
}

static Ast *parse_constexpr(Parser *P) {
  Tok t;
  if (tok_present(t = TRY(P, "-"))) {
    Ast *c = mk_call(t.index, concat2("operator", "-"));
    list_push(&c->u.call.argumentList, parse_term(P));
    return c;
  }
  Ast *left = parse_term(P);
  if (tok_present(t = TRY(P, "."))) {
    Tok id = consume_kind(P, "identifier");
    left = mk_dot(t.index, left, tok_string(id));
  }
  return left;
}

static const char *get_address_space(Parser *P, const char *addressSpace, int *consumed) {
  *consumed = 1;
  if (addressSpace) return addressSpace;
  Tok t = CONSUME(P, "constant", "device", "threadgroup", "thread");
  return tok_string(t);
}

static Ast *parse_type(Parser *P) {
  Tok token;
  const char *addressSpace = NULL;
  int addressSpaceConsumed = 0;
  if (tok_present(token = TRY(P, "constant", "device", "threadgroup", "thread")))
    addressSpace = tok_string(token);
  Tok name = consume_kind(P, "identifier");
  Ast *type = ast_new(A_TYPEREF, name.index);
  type->u.typeRef.name = tok_string(name);
  parse_type_arguments(P, &type->u.typeRef.typeArguments);

  while (tok_present(token = TRY(P, "*", "["))) {
    if (tok_eq(token, "*")) {
      const char *as = get_address_space(P, addressSpace, &addressSpaceConsumed);
      Ast *p = ast_new(A_PTRTYPE, token.index);
      p->u.ref.addressSpace = as;
      p->u.ref.elementType = type;
      type = p;
      continue;
    }
    if (tok_present(TRY(P, "]"))) {
      const char *as = get_address_space(P, addressSpace, &addressSpaceConsumed);
      Ast *r = ast_new(A_ARRAYREFTYPE, token.index);
      r->u.ref.addressSpace = as;
      r->u.ref.elementType = type;
      type = r;
      continue;
    }
    Ast *elem = type;
    Ast *num = parse_constexpr(P);
    Ast *arr = ast_new(A_ARRAYTYPE, token.index);
    arr->u.arrayType.elementType = elem;
    arr->u.arrayType.numElements = num;
    type = arr;
    CONSUME(P, "]");
  }

  if (addressSpace && !addressSpaceConsumed)
    p_fail(P, "Address space specified for type that does not need address space");
  return type;
}

/* ---- operators ---- */
typedef enum { OPK_CALL, OPK_LOGICAL, OPK_EQUALITY } OpKind;
typedef Ast *(*ParseFn)(Parser *P);

static Ast *generic_parse_left(Parser *P, const char *const *texts, ParseFn next, OpKind kind) {
  Ast *left = next(P);
  Tok token;
  while (tok_present(token = try_consume_texts(P, texts))) {
    Ast *right = next(P);
    if (kind == OPK_CALL) {
      Ast *c = mk_call(token.index, concat2("operator", tok_string(token)));
      list_push(&c->u.call.argumentList, left);
      list_push(&c->u.call.argumentList, right);
      left = c;
    } else if (kind == OPK_LOGICAL) {
      Ast *l = ast_new(A_LOGICAL, token.index);
      l->u.logical.text = tok_string(token);
      l->u.logical.left = mk_bool_call(left);
      l->u.logical.right = mk_bool_call(right);
      left = l;
    } else {
      Ast *c = mk_call(token.index, "operator==");
      list_push(&c->u.call.argumentList, left);
      list_push(&c->u.call.argumentList, right);
      if (tok_eq(token, "!=")) {
        Ast *n = ast_new(A_LOGICALNOT, token.index);
        n->u.notExpr.operand = c;
        c = n;
      }
      left = c;
    }
  }
  return left;
}

/* ---- calls, suffixes, increments ---- */
static void scope_call_head(Parser *P) {
  consume_kind(P, "identifier");
  AstList scratch = {0};
  parse_type_arguments(P, &scratch);
  CONSUME(P, "(");
}
static int is_call_expression(Parser *P) { return test_scope(P, scope_call_head); }

static Ast *parse_call_expression(Parser *P) {
  Tok name = consume_kind(P, "identifier");
  Ast *c = mk_call(name.index, tok_string(name));
  parse_type_arguments(P, &c->u.call.typeArguments);
  CONSUME(P, "(");
  while (!tok_present(TEST(P, ")"))) {
    list_push(&c->u.call.argumentList, parse_possible_assignment(P, 0));
    if (!tok_present(TRY(P, ",")))
      break;
  }
  CONSUME(P, ")");
  return c;
}

/* Lexer-free helpers for the RMW nodes: the old and new values are anonymous variables. */
static Ast *emit_increment(Tok token, Ast *old, Ast *extraArg) {
  char name[32];
  snprintf(name, sizeof name, "operator%.*s", token.len, token.text);
  size_t n = strlen(name);
  if (n > 0 && name[n - 1] == '=') name[n - 1] = 0;
  Ast *c = mk_call(token.index, arena_strndup(name, (int)strlen(name)));
  list_push(&c->u.call.argumentList, old);
  if (extraArg) list_push(&c->u.call.argumentList, extraArg);
  return c;
}
/* ReadModifyWriteExpression(token, left): its old and new value variables are the targets of its anon refs */
static Ast *new_rmw(Tok token, Ast *left) {
  Ast *rmw = ast_new(A_RMW, token.index);
  rmw->u.rmw.lValue = left;
  rmw->u.rmw.oldVar = ast_new(A_ANONVAR, token.index);
  rmw->u.rmw.newVar = ast_new(A_ANONVAR, token.index);
  return rmw;
}
static Ast *finish_post_increment(Tok token, Ast *left) {
  Ast *rmw = new_rmw(token, left);
  rmw->u.rmw.newValueExp = emit_increment(token, mk_var_ref(token.index, "anonVar", 1, rmw->u.rmw.oldVar), NULL);
  rmw->u.rmw.resultExp = mk_var_ref(token.index, "anonVar", 1, rmw->u.rmw.oldVar);
  return rmw;
}
static Ast *finish_pre_increment(Tok token, Ast *left, Ast *extraArg) {
  Ast *rmw = new_rmw(token, left);
  rmw->u.rmw.newValueExp = emit_increment(token, mk_var_ref(token.index, "anonVar", 1, rmw->u.rmw.oldVar), extraArg);
  rmw->u.rmw.resultExp = mk_var_ref(token.index, "anonVar", 2, rmw->u.rmw.newVar);
  return rmw;
}

static Ast *parse_suffix_operator(Parser *P, Ast *left, const char *const *ops) {
  Tok token;
  while (tok_present(token = try_consume_texts(P, ops))) {
    if (tok_eq(token, "++") || tok_eq(token, "--"))
      return finish_post_increment(token, left);
    if (tok_eq(token, ".") || tok_eq(token, "->")) {
      if (tok_eq(token, "->")) {
        Ast *d = ast_new(A_DEREF, token.index);
        d->u.deref.ptr = left;
        left = d;
      }
      Tok id = consume_kind(P, "identifier");
      left = mk_dot(token.index, left, tok_string(id));
      continue;
    }
    if (tok_eq(token, "[")) {
      Ast *index = parse_expression(P);
      CONSUME(P, "]");
      Ast *ix = ast_new(A_INDEX, token.index);
      ix->u.index.base = left;
      ix->u.index.index = index;
      left = ix;
      continue;
    }
    wsl_throw(0, "Error", "Bad token");
  }
  return left;
}

static const char *const SUFFIX_ALL[] = {"++", "--", ".", "->", "[", NULL};
static const char *const SUFFIX_LIMITED[] = {".", "->", "[", NULL};
static const char *const SUFFIX_DOT_ONLY[] = {".", NULL};

static Ast *parse_possible_suffix(Parser *P) {
  const char *const *ops = SUFFIX_ALL;
  Ast *left;
  if (is_call_expression(P)) {
    left = parse_call_expression(P);
    ops = SUFFIX_LIMITED;
  } else
    left = parse_term(P);
  return parse_suffix_operator(P, left, ops);
}

static Ast *parse_pre_increment(Parser *P) {
  Tok token = CONSUME(P, "++", "--");
  Ast *left = parse_possible_prefix(P);
  return finish_pre_increment(token, left, NULL);
}

static Ast *parse_possible_prefix(Parser *P) {
  Tok token;
  if (tok_present(TEST(P, "++", "--")))
    return parse_pre_increment(P);
  if (tok_present(token = TRY(P, "+", "-", "~"))) {
    Ast *c = mk_call(token.index, concat2("operator", tok_string(token)));
    list_push(&c->u.call.argumentList, parse_possible_prefix(P));
    return c;
  }
  if (tok_present(token = TRY(P, "*"))) {
    Ast *d = ast_new(A_DEREF, token.index);
    d->u.deref.ptr = parse_possible_prefix(P);
    return d;
  }
  if (tok_present(token = TRY(P, "&"))) {
    Ast *m = ast_new(A_MAKEPTR, token.index);
    m->u.lvalueRef.lValue = parse_possible_prefix(P);
    return m;
  }
  if (tok_present(token = TRY(P, "@"))) {
    Ast *m = ast_new(A_MAKEARRAYREF, token.index);
    m->u.lvalueRef.lValue = parse_possible_prefix(P);
    return m;
  }
  if (tok_present(token = TRY(P, "!"))) {
    Ast *remainder = parse_possible_prefix(P);
    Ast *n = ast_new(A_LOGICALNOT, token.index);
    n->u.notExpr.operand = mk_bool_call(remainder);
    return n;
  }
  return parse_possible_suffix(P);
}

static Ast *parse_possible_product(Parser *P);
static Ast *parse_possible_sum(Parser *P);
static Ast *parse_possible_shift(Parser *P);
static Ast *parse_possible_relational_inequality(Parser *P);
static Ast *parse_possible_relational_equality(Parser *P);
static Ast *parse_possible_bitwise_and(Parser *P);
static Ast *parse_possible_bitwise_xor(Parser *P);
static Ast *parse_possible_bitwise_or(Parser *P);
static Ast *parse_possible_logical_and(Parser *P);

static const char *const OPS_PRODUCT[] = {"*", "/", "%", NULL};
static const char *const OPS_SUM[] = {"+", "-", NULL};
static const char *const OPS_SHIFT[] = {"<<", ">>", NULL};
static const char *const OPS_RELATIONAL[] = {"<", ">", "<=", ">=", NULL};
static const char *const OPS_EQUALITY[] = {"==", "!=", NULL};
static const char *const OPS_AND[] = {"&", NULL};
static const char *const OPS_XOR[] = {"^", NULL};
static const char *const OPS_OR[] = {"|", NULL};
static const char *const OPS_LOGICAL_AND[] = {"&&", NULL};
static const char *const OPS_LOGICAL_OR[] = {"||", NULL};

static Ast *parse_possible_product(Parser *P) { return generic_parse_left(P, OPS_PRODUCT, parse_possible_prefix, OPK_CALL); }
static Ast *parse_possible_sum(Parser *P) { return generic_parse_left(P, OPS_SUM, parse_possible_product, OPK_CALL); }
static Ast *parse_possible_shift(Parser *P) { return generic_parse_left(P, OPS_SHIFT, parse_possible_sum, OPK_CALL); }
static Ast *parse_possible_relational_inequality(Parser *P) { return generic_parse_left(P, OPS_RELATIONAL, parse_possible_shift, OPK_CALL); }
static Ast *parse_possible_relational_equality(Parser *P) { return generic_parse_left(P, OPS_EQUALITY, parse_possible_relational_inequality, OPK_EQUALITY); }
static Ast *parse_possible_bitwise_and(Parser *P) { return generic_parse_left(P, OPS_AND, parse_possible_relational_equality, OPK_CALL); }
static Ast *parse_possible_bitwise_xor(Parser *P) { return generic_parse_left(P, OPS_XOR, parse_possible_bitwise_and, OPK_CALL); }
static Ast *parse_possible_bitwise_or(Parser *P) { return generic_parse_left(P, OPS_OR, parse_possible_bitwise_xor, OPK_CALL); }
static Ast *parse_possible_logical_and(Parser *P) { return generic_parse_left(P, OPS_LOGICAL_AND, parse_possible_bitwise_or, OPK_LOGICAL); }
static Ast *parse_possible_logical_or(Parser *P) { return generic_parse_left(P, OPS_LOGICAL_OR, parse_possible_logical_and, OPK_LOGICAL); }

static Ast *parse_possible_ternary_conditional(Parser *P) {
  Ast *predicate = parse_possible_logical_or(P);
  Tok op = TRY(P, "?");
  if (!tok_present(op)) return predicate;
  /* Parse.js: new TernaryExpression(...), and no such class exists in the row. */
  wsl_throw(0, "ReferenceError", "TernaryExpression is not defined");
  return NULL;
}

static Ast *parse_possible_assignment(Parser *P, int required) {
  Ast *lhs = parse_possible_ternary_conditional(P);
  Tok op = TRY(P, "=", "+=", "-=", "*=", "/=", "%=", "^=", "|=", "&=");
  if (!tok_present(op)) {
    if (required) p_fail(P, "Expected assignment");
    return lhs;
  }
  if (tok_eq(op, "=")) {
    Ast *a = ast_new(A_ASSIGN, op.index);
    a->u.assign.lhs = lhs;
    a->u.assign.rhs = parse_possible_assignment(P, 0);
    return a;
  }
  Ast *rhs = parse_possible_assignment(P, 0);
  return finish_pre_increment(op, lhs, rhs);
}

static Ast *parse_assignment(Parser *P) { return parse_possible_assignment(P, 1); }

static Ast *cb_post_increment(Parser *P, void *ctx) {
  (void)ctx;
  Ast *left = parse_suffix_operator(P, parse_term(P), SUFFIX_DOT_ONLY);
  Tok token = CONSUME(P, "++", "--");
  return finish_post_increment(token, left);
}
static Ast *cb_pre_increment(Parser *P, void *ctx) {
  (void)ctx;
  return parse_pre_increment(P);
}
static Ast *cb_effectful_then_comma(Parser *P, void *ctx) {
  (void)ctx;
  parse_effectful_expression(P);
  CONSUME(P, ",");
  return NULL;   /* JS callback returns undefined */
}

static Ast *parse_effectful_expression(Parser *P) {
  if (is_call_expression(P)) return parse_call_expression(P);
  Ast *pre = backtracking_scope(P, cb_pre_increment, NULL);
  if (pre) return pre;
  Ast *post = backtracking_scope(P, cb_post_increment, NULL);
  if (post) return post;
  return parse_assignment(P);
}

/* genericParseCommaExpression: the effectful attempt never yields a value in the row (its
 * callback returns undefined), so the final expression is always the result. */
static Ast *generic_parse_comma(Parser *P, ParseFn finalParser) {
  Tok origin = P_peek(P);
  if (!tok_present(origin)) p_fail(P, "Unexpected end of file");
  backtracking_scope(P, cb_effectful_then_comma, NULL);
  return finalParser(P);
}
static Ast *parse_possible_assignment_plain(Parser *P) { return parse_possible_assignment(P, 0); }
static Ast *parse_effectful_expression_as_fn(Parser *P) { return parse_effectful_expression(P); }

static Ast *parse_expression(Parser *P) { return generic_parse_comma(P, parse_possible_assignment_plain); }

static Ast *parse_effectful_statement(Parser *P) {
  Ast *result = generic_parse_comma(P, parse_effectful_expression_as_fn);
  CONSUME(P, ";");
  return result;
}

/* ---- statements ---- */
static Ast *cb_variable_decls(Parser *P, void *ctx) {
  (void)ctx;
  return parse_variable_decls(P);
}

static Ast *parse_variable_decls(Parser *P) {
  Ast *type = parse_type(P);
  Ast *list = ast_new(A_COMMA, type->origin);
  Tok sep;
  do {
    Tok name = consume_kind(P, "identifier");
    Ast *init = NULL;
    if (tok_present(TRY(P, "="))) init = parse_expression(P);
    Ast *d = ast_new(A_VARIABLEDECL, name.index);
    d->u.varDecl.name = tok_string(name);
    d->u.varDecl.type = type;
    d->u.varDecl.initializer = init;
    list_push(&list->u.comma.list, d);
    sep = CONSUME(P, ",", ";");
  } while (tok_eq(sep, ","));
  return list;
}

static Ast *parse_block_body(Parser *P, const char *const *terminators) {
  Ast *block = ast_new(A_BLOCK, -1);
  while (!tok_present(test_texts(P, terminators))) {
    Ast *st = parse_statement(P);
    if (st) list_push(&block->u.block.statements, st);
  }
  return block;
}

static Ast *parse_block(Parser *P) {
  CONSUME(P, "{");
  Ast *block = parse_block_body(P, TXT("}"));
  CONSUME(P, "}");
  return block;
}

static Ast *parse_switch_case(Parser *P) {
  Tok token = CONSUME(P, "default", "case");
  Ast *value = NULL;
  if (tok_eq(token, "case")) value = parse_constexpr(P);
  CONSUME(P, ":");
  Ast *body = parse_block_body(P, TXT("}", "default", "case"));
  Ast *c = ast_new(A_SWITCHCASE, token.index);
  c->u.switchCase.value = value;
  c->u.switchCase.body = body;
  return c;
}

static Ast *parse_statement(Parser *P) {
  Tok token = P_peek(P);
  if (!tok_present(token))
    wsl_throw(0, "TypeError", "Cannot read properties of null (reading 'text')");
  if (tok_eq(token, ";")) {
    P_next(P);
    return NULL;
  }
  if (tok_eq(token, "return")) {
    Tok origin = CONSUME(P, "return");
    Ast *r = ast_new(A_RETURN, origin.index);
    if (tok_present(TRY(P, ";"))) return r;
    r->u.ret.value = parse_expression(P);
    CONSUME(P, ";");
    return r;
  }
  if (tok_eq(token, "break")) {
    Tok origin = CONSUME(P, "break");
    CONSUME(P, ";");
    return ast_new(A_BREAK, origin.index);
  }
  if (tok_eq(token, "continue")) {
    Tok origin = CONSUME(P, "continue");
    CONSUME(P, ";");
    return ast_new(A_CONTINUE, origin.index);
  }
  if (tok_eq(token, "while")) {
    Tok origin = CONSUME(P, "while");
    CONSUME(P, "(");
    Ast *cond = parse_expression(P);
    CONSUME(P, ")");
    Ast *body = parse_statement(P);
    Ast *w = ast_new(A_WHILE, origin.index);
    w->u.loop.conditional = mk_bool_call(cond);
    w->u.loop.body = body;
    return w;
  }
  if (tok_eq(token, "do")) {
    Tok origin = CONSUME(P, "do");
    Ast *body = parse_statement(P);
    CONSUME(P, "while");
    CONSUME(P, "(");
    Ast *cond = parse_expression(P);
    CONSUME(P, ")");
    Ast *d = ast_new(A_DOWHILE, origin.index);
    d->u.loop.body = body;
    d->u.loop.conditional = mk_bool_call(cond);
    return d;
  }
  if (tok_eq(token, "for")) {
    Tok origin = CONSUME(P, "for");
    CONSUME(P, "(");
    Ast *init = NULL;
    if (!tok_present(TRY(P, ";"))) {
      init = backtracking_scope(P, cb_variable_decls, NULL);
      if (!init) init = parse_effectful_statement(P);
    }
    Ast *cond = NULL;
    if (!tok_present(TRY(P, ";"))) {
      Ast *c = parse_expression(P);
      CONSUME(P, ";");
      cond = mk_bool_call(c);
    }
    Ast *incr = NULL;
    if (!tok_present(TRY(P, ")"))) {
      incr = parse_expression(P);
      CONSUME(P, ")");
    }
    Ast *body = parse_statement(P);
    Ast *f = ast_new(A_FOR, origin.index);
    f->u.forLoop.initialization = init;
    f->u.forLoop.condition = cond;
    f->u.forLoop.increment = incr;
    f->u.forLoop.body = body;
    return f;
  }
  if (tok_eq(token, "if")) {
    Tok origin = CONSUME(P, "if");
    CONSUME(P, "(");
    Ast *cond = parse_expression(P);
    CONSUME(P, ")");
    Ast *body = parse_statement(P);
    Ast *elseBody = NULL;
    if (tok_present(TRY(P, "else"))) elseBody = parse_statement(P);
    Ast *i = ast_new(A_IF, origin.index);
    i->u.ifStmt.conditional = mk_bool_call(cond);
    i->u.ifStmt.body = body;
    i->u.ifStmt.elseBody = elseBody;
    return i;
  }
  if (tok_eq(token, "switch")) {
    Tok origin = CONSUME(P, "switch");
    CONSUME(P, "(");
    Ast *value = parse_expression(P);
    CONSUME(P, ")");
    CONSUME(P, "{");
    Ast *s = ast_new(A_SWITCHSTATEMENT, origin.index);
    s->u.switchStmt.value = value;
    while (!tok_present(TRY(P, "}")))
      list_push(&s->u.switchStmt.cases, parse_switch_case(P));
    return s;
  }
  if (tok_eq(token, "trap")) {
    Tok origin = CONSUME(P, "trap");
    CONSUME(P, ";");
    return ast_new(A_TRAP, origin.index);
  }
  if (tok_eq(token, "{"))
    return parse_block(P);
  Ast *decl = backtracking_scope(P, cb_variable_decls, NULL);
  if (decl) return decl;
  return parse_effectful_statement(P);
}

/* ---- declarations ---- */
static const char *parse_func_name(Parser *P) {
  if (tok_present(TRY(P, "operator"))) {
    Tok token = CONSUME(P, "+", "-", "*", "/", "%", "^", "&", "|", "<", ">", "<=", ">=", "==", "++", "--", ".", "~", "<<", ">>", "[");
    if (tok_eq(token, "&")) {
      if (tok_present(TRY(P, "["))) {
        CONSUME(P, "]");
        return "operator&[]";
      }
      if (tok_present(TRY(P, "."))) {
        Tok id = consume_kind(P, "identifier");
        return concat2("operator&.", tok_string(id));
      }
      return "operator&";
    }
    if (tok_eq(token, ".")) {
      Tok id = consume_kind(P, "identifier");
      const char *result = concat2("operator.", tok_string(id));
      if (tok_present(TRY(P, "="))) result = concat2(result, "=");
      return result;
    }
    if (tok_eq(token, "[")) {
      CONSUME(P, "]");
      const char *result = "operator[]";
      if (tok_present(TRY(P, "="))) result = "operator[]=";
      return result;
    }
    return concat2("operator", tok_string(token));
  }
  return tok_string(consume_kind(P, "identifier"));
}

typedef struct {
  int origin;
  Ast *returnType;
  const char *name;
  AstList typeParameters;
  int isCast;
  const char *shaderType;
  AstList parameters;
} FuncHead;

static Ast *parse_parameter(Parser *P) {
  Ast *type = parse_type(P);
  Tok name = try_consume_kind(P, "identifier");
  Ast *a = ast_new(A_FUNCPARAMETER, type->origin);
  a->u.named.name = tok_present(name) ? tok_string(name) : NULL;
  a->u.named.type = type;
  return a;
}

static void parse_parameters(Parser *P, AstList *out) {
  CONSUME(P, "(");
  while (!tok_present(TEST(P, ")"))) {
    list_push(out, parse_parameter(P));
    if (!tok_present(TRY(P, ",")))
      break;
  }
  CONSUME(P, ")");
}

static FuncHead parse_func_decl(Parser *P) {
  FuncHead h;
  memset(&h, 0, sizeof h);
  Tok operatorToken = TRY(P, "operator");
  if (tok_present(operatorToken)) {
    h.origin = operatorToken.index;
    parse_type_parameters(P, &h.typeParameters);
    h.returnType = parse_type(P);
    h.name = "operator cast";
    h.isCast = 1;
  } else {
    Tok shaderType = TRY(P, "vertex", "fragment");
    h.returnType = parse_type(P);
    if (tok_present(shaderType)) {
      h.origin = shaderType.index;
      h.shaderType = tok_string(shaderType);
    } else
      h.origin = h.returnType->origin;
    h.name = parse_func_name(P);
    parse_type_parameters(P, &h.typeParameters);
    h.isCast = 0;
  }
  parse_parameters(P, &h.parameters);
  return h;
}

static Ast *func_node(AstTag tag, const FuncHead *h) {
  Ast *a = ast_new(tag, h->origin);
  a->u.func.name = h->name;
  a->u.func.isCast = h->isCast;
  a->u.func.shaderType = h->shaderType;
  a->u.func.isRestricted = 0;
  a->u.func.returnType = h->returnType;
  a->u.func.typeParameters = h->typeParameters;
  a->u.func.parameters = h->parameters;
  return a;
}

static Ast *parse_block_for_body(Parser *P) { return parse_block(P); }

static Ast *parse_func_def(Parser *P) {
  FuncHead h = parse_func_decl(P);
  Ast *body = parse_block_for_body(P);
  Ast *a = func_node(A_FUNCDEF, &h);
  a->u.func.body = body;
  return a;
}

static Ast *parse_protocol_func_decl(Parser *P) {
  FuncHead h = parse_func_decl(P);
  return func_node(A_PROTOCOLFUNCDECL, &h);
}

static Ast *parse_native_func(Parser *P) {
  FuncHead h = parse_func_decl(P);
  CONSUME(P, ";");
  return func_node(A_NATIVEFUNC, &h);
}

static Ast *parse_protocol_decl(Parser *P) {
  Tok origin = CONSUME(P, "protocol");
  Tok name = consume_kind(P, "identifier");
  Ast *a = ast_new(A_PROTOCOLDECL, origin.index);
  a->u.protocolDecl.name = tok_string(name);
  if (tok_present(TRY(P, ":"))) {
    while (!tok_present(TEST(P, "{"))) {
      list_push(&a->u.protocolDecl.extends, parse_protocol_ref(P));
      if (!tok_present(TRY(P, ",")))
        break;
    }
  }
  CONSUME(P, "{");
  while (!tok_present(TRY(P, "}"))) {
    list_push(&a->u.protocolDecl.signatures, parse_protocol_func_decl(P));
    CONSUME(P, ";");
  }
  return a;
}

static Ast *parse_field(Parser *P) {
  Ast *type = parse_type(P);
  Tok name = consume_kind(P, "identifier");
  CONSUME(P, ";");
  Ast *f = ast_new(A_FIELD, name.index);
  f->u.named.name = tok_string(name);
  f->u.named.type = type;
  return f;
}

static Ast *parse_struct_type(Parser *P, Program *prog) {
  (void)prog;
  Tok origin = CONSUME(P, "struct");
  Tok name = consume_kind(P, "identifier");
  Ast *a = ast_new(A_STRUCTTYPE, origin.index);
  a->u.structType.name = tok_string(name);
  parse_type_parameters(P, &a->u.structType.typeParameters);
  CONSUME(P, "{");
  while (!tok_present(TRY(P, "}"))) {
    Ast *f = parse_field(P);
    for (int i = 0; i < a->u.structType.fields.n; i++)
      if (strcmp(a->u.structType.fields.v[i]->u.named.name, f->u.named.name) == 0) {
        char msg[300];
        snprintf(msg, sizeof msg, "Duplicate field name: %s", f->u.named.name);
        type_error_at(P, f->origin, msg);
      }
    list_push(&a->u.structType.fields, f);
  }
  return a;
}

static Ast *parse_enum_member(Parser *P) {
  Tok name = consume_kind(P, "identifier");
  Ast *value = NULL;
  if (tok_present(TRY(P, "="))) value = parse_constexpr(P);
  Ast *m = ast_new(A_ENUMMEMBER, name.index);
  m->u.nameOnly.name = tok_string(name);
  m->u.nameOnly.value = value;
  return m;
}

static Ast *parse_enum_type(Parser *P) {
  CONSUME(P, "enum");
  Tok name = consume_kind(P, "identifier");
  Ast *baseType;
  if (tok_present(TRY(P, ":")))
    baseType = parse_type(P);
  else {
    baseType = ast_new(A_TYPEREF, name.index);
    baseType->u.typeRef.name = "int";
  }
  CONSUME(P, "{");
  Ast *a = ast_new(A_ENUMTYPE, name.index);
  a->u.enumType.name = tok_string(name);
  a->u.enumType.baseType = baseType;
  while (!tok_present(TEST(P, "}"))) {
    Ast *m = parse_enum_member(P);
    for (int i = 0; i < a->u.enumType.members.n; i++)
      if (strcmp(a->u.enumType.members.v[i]->u.nameOnly.name, m->u.nameOnly.name) == 0) {
        char msg[300];
        snprintf(msg, sizeof msg, "Duplicate enum member name: %s", m->u.nameOnly.name);
        type_error_at(P, m->origin, msg);
      }
    list_push(&a->u.enumType.members, m);
    if (!tok_present(TRY(P, ",")))
      break;
  }
  CONSUME(P, "}");
  return a;
}

static Ast *parse_native(Parser *P) {
  Tok origin = CONSUME(P, "native");
  if (tok_present(TRY(P, "typedef"))) {
    Tok name = consume_kind(P, "identifier");
    Ast *a = ast_new(A_NATIVETYPE, origin.index);
    a->u.nativeType.name = tok_string(name);
    parse_type_parameters(P, &a->u.nativeType.typeParameters);
    CONSUME(P, ";");
    return a;
  }
  return parse_native_func(P);
}

static Ast *parse_restricted_func_def(Parser *P) {
  CONSUME(P, "restricted");
  Ast *result;
  if (tok_present(TRY(P, "native")))
    result = parse_native_func(P);
  else
    result = parse_func_def(P);
  result->u.func.isRestricted = 1;
  return result;
}

/* ---- top level: parse(program, origin, originKind, lineNumberOffset, text) ---- */
static void parse_program(Program *prog, const char *origin, int native, int lineOffset, const char *text, int len) {
  AST_ORIGIN_NATIVE = native;
  Parser P_storage;
  Parser *P = &P_storage;
  memset(P, 0, sizeof *P);
  P->L.text = text;
  P->L.len = len;
  P->L.index = 0;
  P->L.native = native;
  P->L.origin = origin;
  P->L.lineOffset = lineOffset;
  for (;;) {
    Tok token = P_peek(P);
    if (!tok_present(token)) return;
    if (tok_eq(token, ";"))
      P_next(P);
    else if (tok_eq(token, "typedef"))
      program_add(P, prog, parse_type_def(P));
    else if (native && tok_eq(token, "native"))
      program_add(P, prog, parse_native(P));
    else if (native && tok_eq(token, "restricted"))
      program_add(P, prog, parse_restricted_func_def(P));
    else if (tok_eq(token, "struct"))
      program_add(P, prog, parse_struct_type(P, prog));
    else if (tok_eq(token, "enum"))
      program_add(P, prog, parse_enum_type(P));
    else if (tok_eq(token, "protocol"))
      program_add(P, prog, parse_protocol_decl(P));
    else
      program_add(P, prog, parse_func_def(P));
  }
}

#endif
