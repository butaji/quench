/* The WSL test runner: executes the operations of WSL_TEST_OPS (wsl-tests-table.h, the format in wsl-tests.h) the way
 * WSL/Test.js runs them. A test's prepare(text) is the pipeline of Prepare.js (the stdlib is cached once, its clone is
 * checked and inlined; a user program is parsed, pruned and checked the same way). callFunction is resolveInlinedFunction
 * (wsl-inliner.h), the parameters are copied as callFunction copies them, and the evaluator runs the instance.
 * Errors are WSL errors (WTypeError, WTrapError, WSyntaxError) or the JS Error of a failed check; checkFail blocks
 * match a class and a message substring as the predicate does.
 *
 * The buffers, typed values, results and lexer tokens of a test live in the test allocator (ta_alloc), not in the
 * AST arena, because each prepare resets the arena. Results of a call are the evaluator's EPtrs (arena), so a result
 * is only valid until the next prepare, which is how the tests use them.
 *
 * tr_run_all(verbose) runs the 285 tests in the table's order and prints the failures; it returns their count. */
#ifndef WSL_TESTS_RUN_H
#define WSL_TESTS_RUN_H

#include <setjmp.h>
#include "wsl-tests.h"
#include "wsl-tests-table.h"

/* ---- the test allocator: reset before each test ---- */
static char *TA_BASE;
static size_t TA_USED, TA_CAP;

static void *ta_alloc(size_t n) {
  n = (n + 15) & ~(size_t)15;
  if (!TA_BASE) {
    TA_CAP = (size_t)64 << 20;
    TA_BASE = malloc(TA_CAP);
    if (!TA_BASE) { fprintf(stderr, "out of memory\n"); exit(4); }
  }
  if (TA_USED + n > TA_CAP) { fprintf(stderr, "test allocator exhausted\n"); exit(6); }
  void *p = TA_BASE + TA_USED;
  TA_USED += n;
  memset(p, 0, n);
  return p;
}

/* ---- the prepared program (Prepare.js) ---- */
static Program TR_STD, TR_IN, TR_OUT;   /* the stdlib (parsed once), the user parse, the checked program */
static size_t TR_MARK;
static int TR_READY;

/* the stdlib is parsed once (standardProgram) */
static void tr_std_parse(void) {
  if (TR_READY) return;
  arena_reset();
  memset(&TR_STD, 0, sizeof TR_STD);
  parse_program(&TR_STD, "/internal/stdlib", 1, 27, INPUTS[0].text, INPUTS[0].len);
  TR_MARK = arena_mark();
  TR_READY = 1;
}

/* a new script context (JetStreamDriver runs each benchmark in its own): the stdlib is parsed again by the next prepare */
static void tr_new_realm(void) { TR_READY = 0; }

/* prepare(origin, lineOffset, text) with a text, or prepare() without one (user = 0). Errors are WSL errors.
 * The pipeline runs at handler depth 1, as prepare_one does: the parser takes WSL_NH > 1 to mean it is inside a
 * backtracking scope, which a checkFail block or the test's own handler must not look like. Its error is then
 * rethrown to the caller's handler. */
static void tr_prepare_body(const char *text, int len, int user);
#define TR_MAXH 64
static void tr_prepare(const char *text, int len, int user) {
  jmp_buf *outer[TR_MAXH];
  int n = WSL_NH;
  memcpy(outer, WSL_HANDLERS, sizeof(jmp_buf *) * (size_t)n);
  jmp_buf jb;
  WSL_NH = 0;
  WSL_HANDLERS[WSL_NH++] = &jb;
  if (setjmp(jb) == 0) {
    tr_prepare_body(text, len, user);
    WSL_NH = 0;
    memcpy(WSL_HANDLERS, outer, sizeof(jmp_buf *) * (size_t)n);
    WSL_NH = n;
    return;
  }
  char cls[32], msg[16400];
  int syntax = WSL_ERR_SYNTAX;
  snprintf(cls, sizeof cls, "%s", WSL_ERR_CLASS);
  snprintf(msg, sizeof msg, "%s", WSL_ERR_MSG);
  WSL_NH = 0;
  memcpy(WSL_HANDLERS, outer, sizeof(jmp_buf *) * (size_t)n);
  WSL_NH = n;
  wsl_throw(syntax, cls, msg);
}

static void tr_prepare_body(const char *text, int len, int user) {
  tr_std_parse();
  arena_reset_to(TR_MARK);
  fi_reset();
  RC_N = 0;
  TDR_N = 0;
  FL_N = 0;
  CUR_ORIGIN = user ? "/internal/test" : "/internal/stdlib";
  CUR_TEXT = user ? text : INPUTS[0].text;
  CUR_LEN = user ? len : INPUTS[0].len;
  ORIGIN_TEXT = CUR_TEXT;
  ORIGIN_LEN = CUR_LEN;
  CUR_LINEOFF = user ? 0 : 27;
  CUR_NATIVE = !user;
  if (!user) {
    prog_clone(&TR_OUT, &TR_STD);
  } else {
    prog_clone(&TR_IN, &TR_STD);
    parse_program(&TR_IN, "/internal/test", 0, 0, text, len);
    program_removed(&TR_OUT, &TR_IN);
  }
  fold_program(&TR_OUT);
  NameCtx *global = program_global_ctx(&TR_OUT);
  PROG_GLOBAL_NC = global;
  names_types(&TR_OUT, global);
  names_protocols(&TR_OUT, global);
  for (int k = 0; k < TR_OUT.statements.n; k++) {
    Ast *st = TR_OUT.statements.v[k];
    if (st->tag == A_TYPEDEF || st->tag == A_STRUCTTYPE || st->tag == A_ENUMTYPE || st->tag == A_NATIVETYPE)
      tdr_visit(st);
  }
  for (int k = 0; k < TR_OUT.statements.n; k++) {
    Ast *st = TR_OUT.statements.v[k];
    if (st->tag == A_PROTOCOLDECL) tdr_visit(st);
  }
  for (int k = 0; k < TR_OUT.statements.n; k++) rc_visit(TR_OUT.statements.v[k]);
  synth_struct_accessors(&TR_OUT, global, TR_OUT.statements.n);
  synth_enum_functions(&TR_OUT, global, TR_OUT.statements.n);
  names_functions(&TR_OUT, global);
  resolve_typedefs_in_functions(&TR_OUT);
  flatten_protocol_extends(&TR_OUT);
  check_program(&TR_OUT);
  post_check_passes(&TR_OUT);
  EBUF_COUNT = 0;
  EBUF_NALL = 0;
  inline_program(&TR_OUT);
}

/* ---- errors: the handler is the top of WSL_HANDLERS; a catcher restores WSL_NH to its own depth ---- */
static const char *WSL_CLASS_NAMES[3] = {"WTypeError", "WTrapError", "WSyntaxError"};
static char FB_CLS[32], FB_MSG[16400];

/* a failed check: a JS Error, which no checkFail predicate matches */
static void tr_fail(const char *msg) { wsl_throw(0, "Error", msg); }

/* ---- the values: a TypedValue is a type and an EPtr (undefined for a void result) ---- */
typedef struct { Ast *type; EPtrV *ptr; } TVal;

#define TR_MAXB 64
#define TR_MAXR 1024
#define TR_MAXP 64
static EBuf *TR_BUFS[TR_MAXB];
static TVal TR_RES[TR_MAXR];
typedef struct { int kind; double d; int res; TVal tv; } PArg;   /* kind: ARG_* or -1 (tv is built) */
static PArg TR_PEND[TR_MAXP];
static int TR_NPEND;
static Tok *TR_LEX;
static int TR_NLEX;

static EBuf *tr_newbuf(int64_t n) {
  EBuf *b = ta_alloc(sizeof(EBuf));
  b->index = -1;
  b->n = n;
  b->cells = ta_alloc(sizeof(Cell) * (size_t)(n ? n : 1));
  return b;
}

static EPtrV *tr_eptr(EBuf *b, int64_t off) {
  EPtrV *p = ta_alloc(sizeof(EPtrV));
  p->buf = b;
  p->off = off;
  return p;
}

/* EPtr.box(value) on a fresh one-cell buffer */
static EPtrV *tr_box(Cell c) {
  EBuf *b = tr_newbuf(1);
  b->cells[0] = c;
  return tr_eptr(b, 0);
}

static Ast *tr_native(const char *name) {
  Ast *t = native_named(&TR_OUT, name);
  if (!t) tr_fail(CAT("No native type ", name));
  return t;
}

/* a named type of the program's global scope */
static Ast *tr_named_type(const char *name) {
  NameEntry *e = nc_get(PROG_GLOBAL_NC, K_TYPE, name);
  if (!e || e->isFunc || !e->thing) tr_fail(CAT("No type named ", name));
  return e->thing;
}

/* The type of a TYPED op's s2: "native typedef int32<>" (the native), "Foo" (a type) or "Foo<int32>" (an instance). */
static Ast *tr_type_of(const char *s) {
  if (!strncmp(s, "native typedef ", 15)) {
    char nm[64];
    const char *p = s + 15;
    int k = 0;
    while (p[k] && p[k] != '<' && k < 63) { nm[k] = p[k]; k++; }
    nm[k] = 0;
    return tr_native(nm);
  }
  const char *lt = strchr(s, '<');
  if (!lt) return tr_named_type(s);
  char base[64], inner[128];
  size_t bl = (size_t)(lt - s);
  if (bl >= sizeof base) tr_fail("type name too long");
  memcpy(base, s, bl);
  base[bl] = 0;
  size_t il = strlen(lt + 1);
  if (il == 0 || lt[1 + il - 1] != '>') tr_fail("bad type instance");
  il--;
  if (il >= sizeof inner) tr_fail("type argument too long");
  memcpy(inner, lt + 1, il);
  inner[il] = 0;
  Ast *st = tr_named_type(base);
  /* TypeRef.instantiate(type, [arg]): a reference to the type with its type arguments */
  Ast **targs = ta_alloc(sizeof(Ast *));
  targs[0] = tr_type_of(inner);
  Ast *r = ast_new(A_TYPEREF, -1);
  r->u.typeRef.name = st->u.structType.name;
  r->u.typeRef.typeArguments.v = targs;
  r->u.typeRef.typeArguments.n = 1;
  r->u.typeRef.typeArguments.cap = 1;
  r->u.typeRef.type = st;
  return r;
}

static Ast *tr_ptr_type(const char *space, Ast *elem) {
  Ast *t = ast_new(A_PTRTYPE, -1);
  t->u.ref.addressSpace = space;
  t->u.ref.elementType = elem;
  return t;
}

static Ast *tr_arrref_type(const char *space, Ast *elem) {
  Ast *t = ast_new(A_ARRAYREFTYPE, -1);
  t->u.ref.addressSpace = space;
  t->u.ref.elementType = elem;
  return t;
}

/* type.size (StructLayoutBuilder first, when the type has no layout yet) */
static int64_t tr_size(Ast *t) {
  int ok;
  int64_t s = ty_size(t, &ok);
  if (!ok) {
    static Buf scratch;
    VTrace st = {&scratch, 0, 0, 1, 0};
    LSB_OFF = 0;
    vt_node(&st, t);
    scratch.n = 0;
    s = ty_size(t, &ok);
    if (!ok) tr_fail("Cannot get size of type");
  }
  return s;
}

/* the number a cell reads as in JS (a boolean is 0 or 1; undefined and null are not numbers) */
static int cell_is_num(Cell c, double d) {
  if (c.kind != CELL_NUM && c.kind != CELL_BOOL) return 0;
  return c.v == d;
}

static double cell_value_num(Cell c) { return c.kind == CELL_NUM || c.kind == CELL_BOOL ? c.v : NAN; }

/* the type of a value's check kind (CHECK_* of wsl-tests.h) matches the result's type */
static int type_matches_check(Ast *type, int kind) {
  Ast *u = ut_unify_node(type);
  switch (kind) {
  case CHECK_INT: return nt_kind(u) == NT_INT32;
  case CHECK_UINT: return nt_kind(u) == NT_UINT32;
  case CHECK_UINT8: return nt_kind(u) == NT_UINT8;
  case CHECK_BOOL: return nt_kind(u) == NT_BOOL;
  case CHECK_FLOAT: return nt_kind(u) == NT_FLOAT32;
  case CHECK_DOUBLE: return nt_kind(u) == NT_FLOAT64;
  case CHECK_ENUM: return u->tag == A_ENUMTYPE;
  case CHECK_NUMBER: return nt_is_number(nt_kind(u));
  }
  return 0;
}

static const char *CHECK_NAMES[8] = {"int", "uint", "uint8", "bool", "float", "double", "enum", "number"};

/* The enum type of argument i of the candidates of name (the callee's parameter), for an ARG_ENUM */
static Ast *tr_enum_param_type(const char *name, int i) {
  NameEntry *e = nc_get(PROG_GLOBAL_NC, K_FUNC, name);
  if (!e) tr_fail(CAT("Cannot resolve function call ", name));
  for (int k = 0; k < e->funcs.n; k++) {
    Ast *f = e->funcs.v[k];
    if (f->u.func.parameters.n <= i) continue;
    Ast *pt = ut_unify_node(f->u.func.parameters.v[i]->u.named.type);
    if (pt->tag == A_ENUMTYPE) return f->u.func.parameters.v[i]->u.named.type;
  }
  tr_fail(CAT("No enum parameter ", name));
  return NULL;
}

/* Turns one pending argument into a TypedValue; name is the callee (for an enum), i the argument's index */
static TVal tr_arg_value(const PArg *a, const char *name, int i) {
  TVal tv;
  if (a->kind == -1) return a->tv;
  if (a->kind == ARG_RES) {
    if (a->res < 0 || a->res >= TR_MAXR) tr_fail("bad result id");
    return TR_RES[a->res];
  }
  Cell c;
  Ast *t;
  switch (a->kind) {
  case ARG_INT: t = tr_native("int32"); c = cnum(a->d); break;
  case ARG_UINT: t = tr_native("uint32"); c = cnum(a->d); break;
  case ARG_UINT8: t = tr_native("uint8"); c = cnum(a->d); break;
  case ARG_BOOL: t = tr_native("bool"); c = cbool(a->d != 0); break;
  case ARG_FLOAT: t = tr_native("float32"); c = cnum(a->d); break;
  case ARG_DOUBLE: t = tr_native("float64"); c = cnum(a->d); break;
  case ARG_ENUM: t = tr_enum_param_type(name, i); c = cnum(a->d); break;
  default: tr_fail("bad argument kind"); t = NULL; c = cundef();
  }
  tv.type = t;
  tv.ptr = tr_box(c);
  return tv;
}

/* The JS Error text of callFunction's resolution failure */
static const char *tr_resolve_message(const char *name, const Overload *ov) {
  const char *s = CAT("Cannot resolve function call ", name, "<>()");
  if (ov->nfails) {
    s = CAT(s, "; tried:\n");
    for (int i = 0; i < ov->nfails; i++) s = CAT(s, i ? "\n" : "", ov->fails[i]);
  }
  return s;
}

/* Evaluator.runFunc(func) for an instance, on the test's side: the value's EPtr, or NULL for no return; a trap or any
 * other error is a WSL error (WTrapError or Error). */
static EPtrV *tr_run_func(Ast *func) {
  jmp_buf jb;
  jmp_buf *saved = EV_JB;
  EV_JB = &jb;
  EV_CF = EV_NONE;
  EV_RETV = NULL;
  if (setjmp(jb) == 0) {
    EPtrV *r = ev_runbody(func->u.func.returnType, func->ePtrAux, func->u.func.body);
    EV_JB = saved;
    return r;
  }
  EV_JB = saved;
  EV_CF = EV_NONE;
  EV_RETV = NULL;
  char msg[1024];
  snprintf(msg, sizeof msg, "%s", EV_MSG);
  wsl_throw(0, EV_ERRK == 1 ? "WTrapError" : "Error", msg);
  return NULL;
}

/* callFunction(program, name, [], args) */
static void tr_call(const WslOp *o) {
  int n = o->c;
  if (n < 0 || n > TR_NPEND) tr_fail("call with too few pending arguments");
  PArg *pend = TR_PEND + (TR_NPEND - n);
  TVal *tv = ta_alloc(sizeof(TVal) * (n ? n : 1));
  Ast **types = ta_alloc(sizeof(Ast *) * (n ? n : 1));
  for (int i = 0; i < n; i++) {
    tv[i] = tr_arg_value(&pend[i], o->s, i);
    types[i] = tv[i].type;
  }
  TR_NPEND -= n;
  Overload ov;
  memset(&ov, 0, sizeof ov);
  Ast *fn = resolve_inlined_function(o->s, NULL, 0, types, n, 1, &ov);
  if (!fn) wsl_throw(0, "WTypeError", tr_resolve_message(o->s, &ov));
  for (int i = 0; i < n; i++) {
    Ast *param = fn->u.func.parameters.v[i];
    /* argumentTypes[i].instantiatedType: InstantiateImmediates, which instantiates a struct reference */
    Ast *itype = rx_visit(rx_new(0, 0, 1, 0, 0), types[i]);
    ev_copy(param->ePtr, tv[i].ptr, tr_size(itype));
  }
  EPtrV *r = tr_run_func(fn);
  Ast *ret = fn->u.func.uninstRet ? fn->u.func.uninstRet : fn->u.func.returnType;
  if (o->b < 0 || o->b >= TR_MAXR) tr_fail("bad result id");
  TR_RES[o->b].type = ret;
  TR_RES[o->b].ptr = r;
}

/* the check of a result value (checkInt..checkDouble, checkNumber, checkEnum) */
static void tr_check(const WslOp *o) {
  if (o->b < 0 || o->b >= TR_MAXR) tr_fail("bad result id");
  TVal r = TR_RES[o->b];
  if (!r.type) tr_fail("check of a void result");
  if (!type_matches_check(r.type, o->a)) {
    char msg[256];
    snprintf(msg, sizeof msg, "Wrong result type for check %s", CHECK_NAMES[o->a]);
    tr_fail(msg);
  }
  Cell c = r.ptr ? ev_load(r.ptr) : cundef();
  if (!cell_is_num(c, o->d)) {
    char msg[256];
    snprintf(msg, sizeof msg, "Wrong result: %g (expected %g)", cell_value_num(c), o->d);
    tr_fail(msg);
  }
}

/* RESVAL: result.value (0 number, 1 boolean, 2 null: an undefined value is equal to null) */
static void tr_resval(const WslOp *o) {
  if (o->a < 0 || o->a >= TR_MAXR) tr_fail("bad result id");
  TVal r = TR_RES[o->a];
  Cell c = r.ptr ? ev_load(r.ptr) : cundef();
  int ok;
  if (o->c == 2) ok = c.kind == CELL_NULL || c.kind == CELL_UNDEF;
  else ok = cell_is_num(c, o->d) && (o->c != 1 || c.kind == CELL_BOOL);
  if (!ok) tr_fail("Wrong result value");
}

/* RGET: result.ePtr.get(index) (0 number, 1 boolean, 2 undefined) */
static void tr_rget(const WslOp *o) {
  if (o->a < 0 || o->a >= TR_MAXR) tr_fail("bad result id");
  TVal r = TR_RES[o->a];
  if (!r.ptr) tr_fail("Null ptr");
  Cell c = ev_get(r.ptr, o->b);
  int ok;
  if (o->c == 2) ok = c.kind == CELL_UNDEF;
  else ok = cell_is_num(c, o->d) && (o->c != 1 || c.kind == CELL_BOOL);
  if (!ok) tr_fail("Wrong result cell");
}

/* a buffer cell check (BGET: buffer.get(index)) */
static void tr_bget(const WslOp *o) {
  if (o->a < 0 || o->a >= TR_MAXB || !TR_BUFS[o->a]) tr_fail("bad buffer id");
  EBuf *b = TR_BUFS[o->a];
  if (o->b < 0 || o->b >= b->n) tr_fail("buffer index out of bounds");
  Cell c = b->cells[o->b];
  int ok;
  if (o->c == 2) ok = c.kind == CELL_UNDEF;
  else ok = cell_is_num(c, o->d) && (o->c != 1 || c.kind == CELL_BOOL);
  if (!ok) tr_fail("Wrong buffer value");
}

/* the values of the test's arguments that are buffers */
static void tr_typed(const WslOp *o, PArg *out) {
  if (o->a < 0 || o->a >= TR_MAXB || !TR_BUFS[o->a]) tr_fail("bad buffer id");
  EBuf *b = TR_BUFS[o->a];
  Ast *elemOrType = tr_type_of(o->s2);
  out->kind = -1;
  out->d = 0;
  out->res = -1;
  if (o->c == TYPED_PTR) {
    Ast *t = tr_ptr_type(o->s, elemOrType);
    out->tv.type = t;
    out->tv.ptr = tr_box(cptr(tr_eptr(b, o->b)));
  } else if (o->c == TYPED_ARRREF) {
    Ast *t = tr_arrref_type(o->s, elemOrType);
    ArrRefV *ar = ta_alloc(sizeof(ArrRefV));
    ar->ptr = tr_eptr(b, o->b);
    ar->len = o->d;
    Cell c = {CELL_ARR, 0, ar};
    out->tv.type = t;
    out->tv.ptr = tr_box(c);
  } else {
    out->tv.type = elemOrType;
    out->tv.ptr = tr_eptr(b, o->b);
  }
}

/* doLex(text): the tokens of the native lexer; a lexing error is a WSyntaxError */
static void tr_lex(const WslOp *o) {
  Lexer L = {o->s, o->len, 0, 1, "/internal/test", 0, NULL};
  Tok t;
  int r;
  int count = 0;
  while ((r = lex_next(&L, &t)) == 1) count++;
  if (r < 0) {
    char msg[600];
    snprintf(msg, sizeof msg, "Syntax error at /internal/test:%d: %s", lex_line_of(&L, L.index), L.errmsg);
    wsl_throw(1, "WSyntaxError", msg);
  }
  TR_LEX = ta_alloc(sizeof(Tok) * (count ? count : 1));
  TR_NLEX = count;
  Lexer L2 = {o->s, o->len, 0, 1, "/internal/test", 0, NULL};
  for (int i = 0; i < count; i++) {
    lex_next(&L2, &TR_LEX[i]);
  }
}

/* checkLexerToken(tokens[b], c, s, s2) */
static void tr_lexcheck(const WslOp *o) {
  if (o->b < 0 || o->b >= TR_NLEX) tr_fail("Cannot read properties of undefined (token)");
  const Tok *t = &TR_LEX[o->b];
  if (t->index != o->c) tr_fail("Wrong lexer index");
  if (strcmp(t->kind, o->s) != 0) tr_fail("Wrong lexer kind");
  if ((size_t)t->len != strlen(o->s2) || memcmp(t->text, o->s2, (size_t)t->len) != 0) tr_fail("Wrong lexer text");
}

/* The catch of a checkFail block: the error must be an instance of its class and contain its substring */
static int fb_matches(const WslOp *o, const char *cls, const char *msg) {
  if (o->a < 0 || o->a > 2) return 0;
  if (strcmp(cls, WSL_CLASS_NAMES[o->a]) != 0) return 0;
  if (o->s && !strstr(msg, o->s)) return 0;
  return 1;
}

static void tr_exec(int first, int end);

/* Runs the operations of [first, end); a checkFail block (FBEGIN .. FEND) runs its operations under a handler. */
static void tr_exec(int first, int end) {
  for (int i = first; i < end; i++) {
    const WslOp *o = &WSL_TEST_OPS[i];
    switch (o->op) {
    case OP_PREP:
      tr_prepare(o->s, o->len, 1);
      break;
    case OP_ARG:
      if (TR_NPEND >= TR_MAXP) tr_fail("too many pending arguments");
      TR_PEND[TR_NPEND].kind = o->a;
      TR_PEND[TR_NPEND].d = o->d;
      TR_PEND[TR_NPEND].res = o->b;
      TR_PEND[TR_NPEND].tv.type = NULL;
      TR_PEND[TR_NPEND].tv.ptr = NULL;
      TR_NPEND++;
      break;
    case OP_TYPED:
      if (TR_NPEND >= TR_MAXP) tr_fail("too many pending arguments");
      tr_typed(o, &TR_PEND[TR_NPEND]);
      TR_NPEND++;
      break;
    case OP_CALL:
      tr_call(o);
      break;
    case OP_CHECK:
      tr_check(o);
      break;
    case OP_FBEGIN: {
      int fe = i + 1;
      while (fe < end && WSL_TEST_OPS[fe].op != OP_FEND) fe++;
      if (fe >= end) tr_fail("checkFail block without its end");
      int nh = WSL_NH;
      jmp_buf jb;
      WSL_HANDLERS[WSL_NH++] = &jb;
      if (setjmp(jb) == 0) {
        tr_exec(i + 1, fe);
        WSL_NH = nh;
        tr_fail("Did not throw exception");
      }
      WSL_NH = nh;
      snprintf(FB_CLS, sizeof FB_CLS, "%s", WSL_ERR_CLASS);
      snprintf(FB_MSG, sizeof FB_MSG, "%s", WSL_ERR_MSG);
      if (!fb_matches(o, FB_CLS, FB_MSG)) wsl_throw(0, FB_CLS, FB_MSG);
      i = fe;
      break;
    }
    case OP_FEND:
      tr_fail("checkFail end without a block");
      break;
    case OP_LEX:
      tr_lex(o);
      break;
    case OP_LEXCHECK:
      tr_lexcheck(o);
      break;
    case OP_BUF:
      if (o->a < 0 || o->a >= TR_MAXB) tr_fail("bad buffer id");
      TR_BUFS[o->a] = tr_newbuf(o->b);
      break;
    case OP_BSET: {
      if (o->a < 0 || o->a >= TR_MAXB || !TR_BUFS[o->a]) tr_fail("bad buffer id");
      Cell c = o->c ? cbool(o->d != 0) : cnum(o->d);
      ebuf_set(TR_BUFS[o->a], o->b, c);
      break;
    }
    case OP_BGET:
      tr_bget(o);
      break;
    case OP_RESVAL:
      tr_resval(o);
      break;
    case OP_RGET:
      tr_rget(o);
      break;
    default:
      tr_fail("unknown operation");
    }
  }
}

/* one test: its operations under the top handler; returns 1 on success, 0 with msg on a failure */
static int tr_run_test(const WslTest *t, char *msg, size_t msgcap) {
  TA_USED = 0;
  TR_NPEND = 0;
  TR_NLEX = 0;
  memset(TR_BUFS, 0, sizeof TR_BUFS);
  memset(TR_RES, 0, sizeof TR_RES);
  jmp_buf top;
  WSL_NH = 0;
  WSL_HANDLERS[WSL_NH++] = &top;
  if (setjmp(top) != 0) {
    WSL_NH = 0;
    snprintf(msg, msgcap, "%s: %s", WSL_ERR_CLASS, WSL_ERR_MSG);
    return 0;
  }
  tr_exec(t->first, t->first + t->count);
  WSL_NH = 0;
  return 1;
}

/* Runs every test of the table (in the table's order, which is doTest's order). Returns the number of failures;
 * with verbose, prints each failure and the total. */
static int tr_run_all(int verbose) {
  int fails = 0;
  const char *only = getenv("WSL_ONLY");   /* debugging: run the test with this name only */
  for (int i = 0; i < WSL_NTESTS; i++) {
    char msg[16600];
    msg[0] = 0;
    if (only && strcmp(only, WSL_TESTS[i].name) != 0) continue;
    if (!tr_run_test(&WSL_TESTS[i], msg, sizeof msg)) {
      fails++;
      if (verbose) printf("FAIL %s: %.300s\n", WSL_TESTS[i].name, msg);
    }
  }
  if (verbose) printf("wsl tests: %d passed, %d failed\n", WSL_NTESTS - fails, fails);
  return fails;
}

#endif
