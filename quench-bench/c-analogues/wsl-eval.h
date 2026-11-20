/* A port of EvaluationCommon.js, Evaluator.js (the combined LHS/RHS evaluator over EPtrs) and the native function
 * bodies of Intrinsics.js that CallExpression runs. The JS exceptions are the control flow of the evaluator here:
 * Return, Break and Continue set EV_CF, and each statement list, loop and switch stops when a flag is set (a loop
 * or switch takes its own Break and Continue, and a function body takes its Return: _runBody). A WTrapError and any
 * other JS error are longjmp'd to the top of the function being run (eval_run_func), with the message kept; the
 * --eval output prints a trap's message and reports any other error as "err".
 *
 * Cells hold what the JS cells hold: undefined, null, a number, a boolean, an EPtr (CELL_PTR) or an EArrayRef
 * (CELL_ARR). The Visitor's default visits of the nodes Evaluator does not override are not ported: a node without an
 * evaluation is a fatal error here (none is reached by the --eval inputs).
 *
 * eval_dump(b, p) is the --eval mode of wsl-ast-dump: after inline(), one line per non-generic function of inline()'s
 * first loop that is a FuncDef without parameters ("eval <index> <name> <value>"), joined by newlines with no trailing
 * newline. */
#ifndef WSL_EVAL_H
#define WSL_EVAL_H

#include <math.h>
#include <setjmp.h>
#include "wsl-intrinsics.h"

enum { EV_NONE = 0, EV_RETURN = 1, EV_BREAK = 2, EV_CONTINUE = 3 };
static int EV_CF;              /* the control flow in flight (Return, Break, Continue) */
static EPtrV *EV_RETV;         /* the value of the Return in flight (NULL for a bare return) */
static jmp_buf *EV_JB;         /* the top of the function being run */
static int EV_ERRK;            /* 1: a WTrapError, 2: any other error */
static const char *EV_MSG;     /* the trap's message, or the error's text */

__attribute__((noreturn)) static void ev_fail(const char *msg) { EV_ERRK = 2; EV_MSG = msg; longjmp(*EV_JB, 1); }
__attribute__((noreturn)) static void ev_trap(const char *msg) { EV_ERRK = 1; EV_MSG = msg; longjmp(*EV_JB, 1); }

static EPtrV *ev(Ast *n);

/* type.size (null, a type without a size, is an error) */
static int64_t ev_size(const Ast *t) {
  int ok;
  int64_t s = ty_size(t, &ok);
  if (!ok) ev_fail("Cannot get size of type");
  return s;
}

/* EBuffer.get / EPtr.get: a cell of the buffer, with the JS bounds check */
static Cell ev_get(const EPtrV *p, int64_t i) {
  int64_t k = p->off + i;
  if (k < 0 || k >= p->buf->n) ev_fail("Out of bounds buffer access");
  return p->buf->cells[k];
}

static void ev_put(EBuf *b, int64_t k, Cell c) {
  if (k < 0 || k >= b->n) ev_fail("out of bounds buffer access");
  b->cells[k] = c;
}

/* EPtr.loadValue (a null EPtr is a TypeError) */
static Cell ev_load(const EPtrV *p) {
  if (!p) ev_fail("loadValue of a missing EPtr");
  return ev_get(p, 0);
}

/* the value of an expression that the JS calls a method on: a missing EPtr is a TypeError */
static EPtrV *ev_req(Ast *n) {
  EPtrV *p = ev(n);
  if (!p) ev_fail("missing EPtr");
  return p;
}

/* EPtr.copyFrom(other, size): from the last cell down; a missing source only fails when a cell is read */
static void ev_copy(EPtrV *dst, const EPtrV *src, int64_t size) {
  for (int64_t i = size; i-- > 0;) {
    if (!src) ev_fail("copyFrom of a missing EPtr");
    Cell c = ev_get(src, i);
    if (!dst) ev_fail("copyFrom to a missing EPtr");
    ev_put(dst->buf, dst->off + i, c);
  }
}

/* Evaluator._snapshot(type, dstPtr, srcPtr): a new buffer when dst is NULL */
static EPtrV *ev_snapshot(const Ast *type, EPtrV *dst, const EPtrV *src) {
  int64_t size = ev_size(type);
  if (!dst) dst = eptr_new(ebuf_new(size), 0);
  ev_copy(dst, src, size);
  return dst;
}

/* EPtr.box(value) of a node's own EPtr: sets cell 0 of its buffer and returns the EPtr */
static EPtrV *ev_box(EPtrV *box, Cell c) {
  if (!box) ev_fail("box of a missing EPtr");
  ev_put(box->buf, 0, c);
  return box;
}

/* EPtr.box(value) on a fresh buffer */
static EPtrV *ev_fresh_box(Cell c) {
  EBuf *b = ebuf_new(1);
  b->cells[0] = c;
  return eptr_new(b, 0);
}

static Cell cnum(double v) { Cell c = {CELL_NUM, v, NULL}; return c; }
static Cell cbool(int v) { Cell c = {CELL_BOOL, v ? 1 : 0, NULL}; return c; }
static Cell cnull(void) { Cell c = {CELL_NULL, 0, NULL}; return c; }
static Cell cundef(void) { Cell c = {CELL_UNDEF, 0, NULL}; return c; }
static Cell cptr(EPtrV *p) { Cell c = {CELL_PTR, 0, p}; return c; }

/* JS truthiness of a cell value */
static int ev_truthy(Cell c) {
  switch (c.kind) {
  case CELL_NUM: return c.v != 0 && c.v == c.v;
  case CELL_BOOL: return c.v != 0;
  case CELL_PTR: case CELL_ARR: return 1;
  default: return 0;
  }
}

static double ev_num(Cell c) {
  if (c.kind != CELL_NUM) ev_fail("expected a number");
  return c.v;
}

/* the cell values a JS comparison or boolean operator reads, through a native's parameters */
static double ld_num(const EPtrV *p) { return ev_num(ev_load(p)); }
static int ld_bool(const EPtrV *p) {
  Cell c = ev_load(p);
  if (c.kind != CELL_BOOL) ev_fail("expected a boolean");
  return c.v != 0;
}

/* Evaluator.runFunc / _runBody(type, ptr, block): the block's Return is snapshotted into ptr; a block that ends
 * without a Return gives NULL (the JS null). */
static EPtrV *ev_runbody(const Ast *type, EPtrV *ptr, Ast *block) {
  if (!ptr) ev_fail("Null ptr");
  ev(block);
  if (EV_CF == EV_RETURN) {
    EV_CF = EV_NONE;
    EPtrV *v = EV_RETV;
    EV_RETV = NULL;
    return ev_snapshot(type, ptr, v);
  }
  if (EV_CF != EV_NONE) ev_fail("Should not see break/continue at function scope");
  return NULL;
}

/* after a loop body: 1 leaves the loop (a Break, or a Return in flight), 0 goes on (none, or a Continue) */
static int ev_loop_exit(void) {
  if (EV_CF == EV_BREAK) { EV_CF = EV_NONE; return 1; }
  if (EV_CF == EV_CONTINUE) { EV_CF = EV_NONE; return 0; }
  return EV_CF != EV_NONE;
}

/* ---- the native function bodies (Intrinsics.js implementation of each NativeFunc) ---- */

/* the NativeType kind of a type node (through its TypeRefs) */
static NtKind ev_kind(const Ast *t) { return nt_kind(ut_unify_node((Ast *)t)); }

/* the value of a shift count: ToUint32(right) & 31 */
static uint32_t ev_shift(double right) { return ((uint32_t)js_uint32(right)) & 31u; }

/* The implementation of the NativeFunc whose instance is nfi, called from call (for its CallExpression fields) with
 * the snapshots of its arguments. Returns the value of the result cell. */
static Cell ev_native(Ast *nfi, Ast *call, EPtrV **args) {
  const char *name = nfi->u.func.name;
  NtKind rk = ev_kind(nfi->u.func.returnType);
  int np = nfi->u.func.parameters.n;
  if (nfi->u.func.isCast) {
    if (np != 1) ev_fail("cast with no single argument");
    double x = ld_num(args[0]);
    switch (rk) {
    case NT_INT32: return cnum(js_int32(x));
    case NT_UINT32: return cnum(js_uint32(x));
    case NT_UINT8: return cnum(js_uint8(x));
    case NT_FLOAT32: return cnum((double)(float)x);
    case NT_FLOAT64: return cnum(x);
    default: ev_fail("cast to an unsupported type");
    }
  }
  /* SynthesizeEnumFunctions.js: the value and the casts of an enum are the identity on the cell */
  if (!strcmp(name, "operator.value") || !strcmp(name, "operator cast")) return ev_load(args[0]);
  if (!strcmp(name, "operator.length")) {
    Cell r = ev_load(args[0]);
    if (r.kind == CELL_ARR) return cnum(((ArrRefV *)r.p)->len);
    return cnum(0);
  }
  if (!strcmp(name, "operator&[]")) {
    Cell r = ev_load(args[0]);
    if (r.kind != CELL_ARR) {
      if (r.kind == CELL_NULL || r.kind == CELL_UNDEF) ev_trap("Null dereference");
      ev_fail("index of a non-reference");
    }
    ArrRefV *ref = r.p;
    double index = ld_num(args[1]);
    if (index > ref->len)
      ev_trap(cat(5, "Array index ", js_num(index), " is out of bounds of ", arrref_str(ref), ""));
    if (!ref->ptr) ev_fail("plus of a null EPtr");
    int64_t size = ev_size(call->u.call.instTA.v[0]);
    return cptr(eptr_new(ref->ptr->buf, ref->ptr->off + (int64_t)index * size));
  }
  if (np < 1 || np > 2) ev_fail("native with an unsupported arity");
  NtKind k = ev_kind(nfi->u.func.parameters.v[0]->u.named.type);
  if (np == 1) {
    if (!strcmp(name, "operator~")) {
      if (k == NT_INT32) return cnum((double)(~(int32_t)js_int32(ld_num(args[0]))));
      if (k == NT_UINT32) return cnum((double)(uint32_t)(~(int32_t)js_int32(ld_num(args[0]))));
    }
    ev_fail("unsupported unary native");
  }
  if (!strcmp(name, "operator==") && k == NT_BOOL) return cbool(ld_bool(args[0]) == ld_bool(args[1]));
  double a = ld_num(args[0]), b = ld_num(args[1]);
  if (!strcmp(name, "operator==")) return cbool(a == b);
  if (!strcmp(name, "operator<")) return cbool(a < b);
  if (!strcmp(name, "operator<=")) return cbool(a <= b);
  if (!strcmp(name, "operator>")) return cbool(a > b);
  if (!strcmp(name, "operator>=")) return cbool(a >= b);
  int isUint = k == NT_UINT32;
  int isInt = k == NT_INT32;
  int isFloat = k == NT_FLOAT32;
  int isDouble = k == NT_FLOAT64;
  if (!isInt && !isUint && !isFloat && !isDouble) ev_fail("arithmetic on an unsupported type");
  if (!strcmp(name, "operator+") || !strcmp(name, "operator-") || !strcmp(name, "operator*") ||
      !strcmp(name, "operator/")) {
    char op = name[8];
    double v = op == '+' ? a + b : op == '-' ? a - b : op == '*' ? a * b : a / b;
    if (isInt) return cnum(js_int32(v));
    if (isUint) return cnum(js_uint32(v));
    if (isFloat) return cnum((double)(float)v);
    return cnum(v);
  }
  if (!strcmp(name, "operator&") || !strcmp(name, "operator|") || !strcmp(name, "operator^")) {
    int32_t x = (int32_t)js_int32(a), y = (int32_t)js_int32(b);
    int32_t r = name[8] == '&' ? (x & y) : name[8] == '|' ? (x | y) : (x ^ y);
    return cnum(isUint ? (double)(uint32_t)r : (double)r);
  }
  if (!strcmp(name, "operator<<")) {
    uint32_t s = ev_shift(b);
    uint32_t r = (uint32_t)(int32_t)js_int32(a) << s;
    return cnum(isUint ? (double)r : (double)(int32_t)r);
  }
  if (!strcmp(name, "operator>>")) {
    uint32_t s = ev_shift(b);
    if (isUint) return cnum((double)((uint32_t)js_uint32(a) >> s));
    return cnum((double)((int32_t)js_int32(a) >> s));
  }
  ev_fail("unknown native function");
  return cundef();
}

/* The synthesized struct accessors (SynthesizeStructAccessors.js) with the layout data of their instance: the getter
 * "operator.<field>" copies the field into a new buffer, the setter "operator.<field>=" copies the struct and then
 * the field, and the ander "operator&.<field>" is a pointer to the field. */
static EPtrV *ev_accessor(Ast *nfi, EPtrV **args) {
  const char *name = nfi->u.func.name;
  int64_t off = nfi->u.func.implOffset, ssize = nfi->u.func.implStructSize, fsize = nfi->u.func.implFieldSize;
  if (!strncmp(name, "operator&.", 10)) {
    Cell b = ev_load(args[0]);
    /* the JS names `node`, which the accessor's closure does not have: a ReferenceError (not a trap) */
    if (b.kind != CELL_PTR) ev_fail("ReferenceError: node is not defined");
    EPtrV *base = b.p;
    return ev_fresh_box(cptr(eptr_new(base->buf, base->off + off)));
  }
  size_t nl = strlen(name);
  if (nl > 0 && name[nl - 1] == '=') {
    EBuf *rb = ebuf_new(ssize);
    ev_copy(eptr_new(rb, 0), args[0], ssize);
    ev_copy(eptr_new(rb, off), args[1], fsize);
    return eptr_new(rb, 0);
  }
  EBuf *rb = ebuf_new(fsize);
  ev_copy(eptr_new(rb, 0), eptr_new(args[0]->buf, args[0]->off + off), fsize);
  return eptr_new(rb, 0);
}

/* ---- the visitor ---- */

/* The switch value's comparison (SwitchStatement: node.type.unifyNode.valuesEqual) */
static int ev_switch_equal(const Ast *switchNode, double a, double b) {
  Ast *t = ut_unify_node(switchNode->type);
  if (t->tag == A_ENUMTYPE) t = ut_unify_node(t->u.enumType.baseType);
  return nt_values_equal(nt_kind(t), a, b);
}

/* SwitchStatement: from the first matching case (or the default), the bodies run in order until a Break */
static void ev_switch(Ast *n) {
  Cell v = ev_load(ev_req(n->u.switchStmt.value));
  AstList *cases = &n->u.switchStmt.cases;
  int start = -1;
  for (int i = 0; i < cases->n && start < 0; i++) {
    Ast *sc = cases->v[i];
    if (!sc->u.switchCase.value) continue;   /* isDefault */
    if (v.kind != CELL_NUM) ev_fail("switch value is not a number");
    if (ev_switch_equal(n, v.v, value_for_selected(unify_of(sc->u.switchCase.value)))) start = i;
  }
  for (int i = 0; start < 0 && i < cases->n; i++)
    if (!cases->v[i]->u.switchCase.value) start = i;
  if (start < 0) ev_fail("Switch statement did not find case");
  for (int j = start; j < cases->n; j++) {
    ev(cases->v[j]->u.switchCase.body);
    if (EV_CF == EV_BREAK) { EV_CF = EV_NONE; break; }
    if (EV_CF != EV_NONE) break;
  }
}

static EPtrV *ev_call(Ast *n) {
  Ast *nfi = n->u.call.nfi;
  if (!nfi) ev_fail("call that is not native");
  int na = n->u.call.argumentList.n;
  EPtrV **vals = eb_alloc(sizeof(EPtrV *) * (na + 1));
  for (int i = 0; i < na; i++) {
    if (i >= nfi->u.func.parameters.n) ev_fail("argument without a parameter type");
    Ast *argument = n->u.call.argumentList.v[i];
    vals[i] = ev(argument);
    if (!vals[i]) ev_fail("Null argument value");
  }
  /* the snapshots of the arguments are taken after all of them are evaluated */
  EPtrV **snaps = eb_alloc(sizeof(EPtrV *) * (na + 1));
  for (int i = 0; i < na; i++)
    snaps[i] = ev_snapshot(nfi->u.func.parameters.v[i]->u.named.type, NULL, vals[i]);
  EPtrV *res = nfi->u.func.implHas ? ev_accessor(nfi, snaps) : ev_fresh_box(ev_native(nfi, n, snaps));
  return ev_snapshot(nfi->u.func.returnType, n->ePtrAux, res);
}

static EPtrV *ev(Ast *n) {
  switch (n->tag) {
  case A_BLOCK:
    for (int i = 0; i < n->u.block.statements.n; i++) {
      ev(n->u.block.statements.v[i]);
      if (EV_CF != EV_NONE) break;
    }
    return NULL;
  case A_FUNCTIONLIKEBLOCK: {
    AstList *args = &n->u.fblock.argumentList;
    for (int i = 0; i < args->n; i++) {
      Ast *p = n->u.fblock.parameters.v[i];
      if (!p->ePtr) ev_fail("missing EPtr");
      EPtrV *src = ev(args->v[i]);
      ev_copy(p->ePtr, src, ev_size(p->u.named.type));
    }
    return ev_runbody(n->u.fblock.returnType, n->ePtrAux, n->u.fblock.body);
  }
  case A_RETURN:
    EV_RETV = n->u.ret.value ? ev(n->u.ret.value) : NULL;
    EV_CF = EV_RETURN;
    return NULL;
  case A_VARIABLEDECL: {
    EPtrV *p = n->ePtr;
    if (!p || !p->buf) ev_fail("eptr without buffer");
    populate_default(n->u.varDecl.type, p->buf, p->off);
    if (n->u.varDecl.initializer) {
      EPtrV *src = ev(n->u.varDecl.initializer);
      ev_copy(p, src, ev_size(n->u.varDecl.type));
    }
    return NULL;
  }
  case A_ASSIGN: {
    EPtrV *target = ev_req(n->u.assign.lhs);
    EPtrV *source = ev(n->u.assign.rhs);
    ev_copy(target, source, ev_size(n->type));
    return target;
  }
  case A_IDENTITY:
    return ev(n->u.identity.target);
  case A_DEREF: {
    Cell c = ev_load(ev_req(n->u.deref.ptr));
    if (c.kind == CELL_PTR) return c.p;
    if (c.kind == CELL_NULL || c.kind == CELL_UNDEF) ev_trap("Null dereference");
    ev_fail("dereference of a non-pointer");
    return NULL;
  }
  case A_MAKEPTR: {
    EPtrV *p = ev(n->u.lvalueRef.lValue);
    return ev_box(n->ePtr, p ? cptr(p) : cundef());
  }
  case A_MAKEARRAYREF: {
    EPtrV *p = ev(n->u.lvalueRef.lValue);
    Cell len = ev_load(ev_req(n->u.lvalueRef.numElements));
    ArrRefV *r = eb_alloc(sizeof(ArrRefV));
    r->ptr = p;
    r->len = ev_num(len);
    Cell c = {CELL_ARR, 0, r};
    return ev_box(n->ePtr, c);
  }
  case A_CONVERTPTR: {
    Cell c = ev_load(ev_req(n->u.lvalueRef.lValue));
    ArrRefV *r = eb_alloc(sizeof(ArrRefV));
    r->ptr = c.kind == CELL_PTR ? c.p : NULL;
    r->len = 1;
    Cell out = {CELL_ARR, 0, r};
    return ev_box(n->ePtr, out);
  }
  case A_COMMA: {
    EPtrV *result = NULL;
    for (int i = 0; i < n->u.comma.list.n; i++) result = ev(n->u.comma.list.v[i]);
    return result;
  }
  case A_VARIABLEREF:
    return n->u.varRef.variable->ePtr;
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL:
  case A_ENUMLITERAL:
    return ev_box(n->ePtr, cnum(value_for_selected(n)));
  case A_NULLLITERAL:
    return ev_box(n->ePtr, cnull());
  case A_BOOLLITERAL:
    return ev_box(n->ePtr, cbool(n->u.boolLit.value));
  case A_LOGICALNOT: {
    Cell c = ev_load(ev_req(n->u.notExpr.operand));
    return ev_box(n->ePtr, cbool(!ev_truthy(c)));
  }
  case A_LOGICAL: {
    Cell lhs = ev_load(ev_req(n->u.logical.left));
    Cell rhs = ev_load(ev_req(n->u.logical.right));
    Cell r;
    if (!strcmp(n->u.logical.text, "&&")) r = ev_truthy(lhs) ? rhs : lhs;
    else if (!strcmp(n->u.logical.text, "||")) r = ev_truthy(lhs) ? lhs : rhs;
    else ev_fail("Unknown type of logical expression");
    return ev_box(n->ePtr, r);
  }
  case A_IF:
    if (ev_truthy(ev_load(ev_req(n->u.ifStmt.conditional))))
      return ev(n->u.ifStmt.body);
    else if (n->u.ifStmt.elseBody)
      return ev(n->u.ifStmt.elseBody);
    return NULL;
  case A_WHILE:
    while (ev_truthy(ev_load(ev_req(n->u.loop.conditional)))) {
      ev(n->u.loop.body);
      if (ev_loop_exit()) break;
    }
    return NULL;
  case A_DOWHILE:
    for (;;) {
      ev(n->u.loop.body);
      if (ev_loop_exit()) break;
      if (!ev_truthy(ev_load(ev_req(n->u.loop.conditional)))) break;
    }
    return NULL;
  case A_FOR:
    if (n->u.forLoop.initialization) ev(n->u.forLoop.initialization);
    for (;;) {
      if (n->u.forLoop.condition && !ev_truthy(ev_load(ev_req(n->u.forLoop.condition)))) break;
      ev(n->u.forLoop.body);
      if (ev_loop_exit()) break;
      if (n->u.forLoop.increment) ev(n->u.forLoop.increment);
    }
    return NULL;
  case A_SWITCHSTATEMENT:
    ev_switch(n);
    return NULL;
  case A_BREAK:
    EV_CF = EV_BREAK;
    return NULL;
  case A_CONTINUE:
    EV_CF = EV_CONTINUE;
    return NULL;
  case A_TRAP:
    ev_trap("Trap statement");
    return NULL;
  case A_ANONVAR: {
    EPtrV *p = n->ePtr;
    if (!p || !p->buf) ev_fail("missing EPtr");
    populate_default(n->type, p->buf, p->off);
    return NULL;
  }
  case A_CALL:
    return ev_call(n);
  default:
    ev_fail("no evaluation for this node");
  }
  return NULL;
}

/* describe a cell value for the --eval output (see the driver's __evalDescribe) */
static void ev_describe(Buf *out, Cell c) {
  switch (c.kind) {
  case CELL_UNDEF: buf_str(out, "undef"); return;
  case CELL_NULL: buf_str(out, "null"); return;
  case CELL_BOOL: buf_str(out, c.v != 0 ? "b1" : "b0"); return;
  case CELL_NUM: {
    if (c.v != c.v) { buf_str(out, "nan"); return; }
    uint64_t bits;
    memcpy(&bits, &c.v, sizeof bits);
    buf_fmt(out, "n%016llx", (unsigned long long)bits);
    return;
  }
  case CELL_PTR: buf_fmt(out, "ptr %s", eptr_str(c.p)); return;
  case CELL_ARR: buf_fmt(out, "arr %s", arrref_str(c.p)); return;
  }
}

/* runFunc(func) for one FuncDef instance: the value of its return, or the trap or error it ends with */
static void ev_run_func(Buf *out, Ast *func) {
  jmp_buf jb;
  jmp_buf *saved = EV_JB;
  EV_JB = &jb;
  EB_OWN = 1;
  EV_CF = EV_NONE;
  EV_RETV = NULL;
  if (setjmp(jb) == 0) {
    EPtrV *r = ev_runbody(func->u.func.returnType, func->ePtrAux, func->u.func.body);
    if (!r) buf_str(out, "fell");
    else ev_describe(out, ev_load(r));
  } else {
    EV_CF = EV_NONE;
    EV_RETV = NULL;
#ifdef WSL_EVAL_DEBUG
    fprintf(stderr, "eval error in %s: %s\n", func->u.func.name, EV_MSG);
#endif
    if (EV_ERRK == 1) buf_fmt(out, "trap %s", EV_MSG);
    else buf_str(out, "err");
  }
  EV_JB = saved;
  EB_OWN = 0;
  eb_reset();
}

/* --eval: the lines of every non-generic FuncDef without parameters, in inline()'s first loop order */
static void eval_dump(Buf *out, const Program *p) {
  Ast **fs = arena_alloc(sizeof(Ast *) * (p->statements.n + 1));
  int n = top_funcs(p, fs);
  int first = 1;
  for (int i = 0; i < n; i++) {
    Ast *inst = fi_get_unique(fs[i], NULL, 0);
    if (inst->tag != A_FUNCDEF || inst->u.func.parameters.n) continue;
    if (!first) buf_str(out, "\n");
    first = 0;
    buf_fmt(out, "eval %d %s ", i, fs[i]->u.func.name);
    ev_run_func(out, inst);
  }
}

#endif
