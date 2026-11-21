/* WSL Checker (a C port of WSL/Checker.js, Check.js, CallExpression.js, AutoWrapper/equalsWithCommit,
 * TypeVariableTracker.js and the property access of PropertyAccessExpression.js). check_program visits
 * the statements in the JS order: types, protocols, then each function's signature and its body.
 * Every expression the checker visits is traced in visit order (post-order): "E class origin type", and a
 * call also "C name overload -> result". Type errors are WTypeError (wtype_error), other failures Error.
 * The trace is the --check output of tools/wsl-ast-dump.cjs; both print the same lines. */
#ifndef WSL_CHECK_H
#define WSL_CHECK_H

#include "wsl-types.h"

/* ---- errors ---- */
/* typeErrorMessage of the WTypeError being raised; read by the handler that catches it */
static const char *CHK_RAW;

/* new WTypeError(origin.originString, msg): the message carries the "Type error at" prefix */
static void wtype_error(const char *origin, const char *msg) {
  CHK_RAW = msg;
  wsl_throw(0, "WTypeError", cat(4, "Type error at ", origin, ": ", msg));
}

/* WTypeError(origin, undefined): the second argument is missing, so the message reads "undefined" */
static void wtype_error_undef(const char *origin) {
  CHK_RAW = "undefined";
  wsl_throw(0, "WTypeError", cat(4, "Type error at ", origin, ": ", "undefined"));
}

/* throw new Error(msg) for a checker invariant */
static void chk_error(const char *msg) { wsl_throw(0, "Error", msg); }

/* Runs fn(cx) under a handler for WTypeError. Returns 1 when it completes; 0 when a WTypeError was raised,
 * with its message in *raw. Any other error propagates. */
typedef void (*ChkFn)(void *cx);
static int try_wtype(ChkFn fn, void *cx, const char **raw) {
  jmp_buf jb;
  int depth = WSL_NH;
  WSL_HANDLERS[WSL_NH++] = &jb;
  if (setjmp(jb) == 0) {
    fn(cx);
    WSL_NH = depth;
    return 1;
  }
  WSL_NH = depth;
  if (strcmp(WSL_ERR_CLASS, "WTypeError") != 0) rethrow_outer();
  *raw = CHK_RAW;
  return 0;
}

/* ---- the trace ---- */
static Buf CHK_TRACE;
static void trace_line(const char *s) {
  buf_str(&CHK_TRACE, s);
  buf_str(&CHK_TRACE, "\n");
}
static const char *origin_tag(const Ast *n) {
  if (n->origin < 0) return "-";
  return cat(2, n->native ? "n" : "u", js_num(n->origin));
}

/* ---- the checker state (Checker.js) ---- */
typedef struct {
  Program *p;
  Ast *stmt;                   /* _currentStatement */
  const char **vertex; int nvertex;
  const char **fragment; int nfragment;
} Checker;

/* prototypes of this file's functions, in definition order (the passes call each other) */
static void wtype_error(const char *origin, const char *msg);
static void wtype_error_undef(const char *origin);
static int try_wtype(ChkFn fn, void *cx, const char **raw);
static void trace_line(const char *s);
static const char *origin_tag(const Ast *n);
static int name_in(const char **v, int n, const char *s);
static int is_expression(const Ast *n);
static int is_literal_expr(const Ast *n);
static int is_constexpr(const Ast *n);
static Ast *var_type(const Ast *v);
static int is_lvalue(const Ast *n);
static const char *addr_space(const Ast *n);
static const char *not_lvalue_string(const Ast *n);
static int equals_with_commit(Ast *a, Ast *b);
static int commit_ctx_nodes(UCtx *c);
static Ast *commit_node(Ast *t);
static double value_for_selected(const Ast *lit);
static void uc_add_extra(UCtx *c, Ast *t);
static Ast *mk_ptr_type(const Ast *src, const char *space, Ast *elem);
static Ast *mk_arrayref_type(const Ast *src, const char *space, Ast *elem);
static Ast *mk_int_lit_typed(const Ast *src, AstTag tag, double value, Ast *type);
static AstList alist(Ast **v, int n);
static void alist_append(AstList *dst, const AstList *src);
static AstList *funcs_named(Program *p, const char *name);
static AstList *signatures_with_tv(Ast *pd, const char *name, Ast *tv);
static Ast *mk_call_node(const Ast *src, const char *name);
static Ast *resolve_to_overload(Ast *call, Overload *ov);
static const char *fails_joined(const char **f, int n);
static Ast *call_resolve(Ast *call, AstList *possible, Ast **scope, int nscope, Ast **tv, int ntv);
static void static_resolve_run(void *cx);
static const char *prop_name(Ast *n, int which);
static Ast *arg_type_for_and(Ast *origin, Ast *t);
static Ast *arg_for_and(Ast *origin, Ast *t, Ast *value);
static Ast *ret_from_and(Ast *origin, Ast *t);
static void prop_try_get(void *cx);
static void prop_try_and(void *cx);
static void prop_try_set(void *cx);
static Ast *finish_prop(Checker *c, Ast *n, Ast *baseType, Ast **extra, Ast **extraT, int nextra);
static AstList *stmt_type_params(Checker *c);
static void tvt_consider(AstList *set, Ast *x);
static void tvt_visit(Ast *n, AstList *set);
static void trace_expr(Ast *n, Ast *r);
static Ast *chk(Checker *c, Ast *n);
static void visit_func_default(Checker *c, Ast *n);
static int text_is_identifier(const char *s);
static void wrong_param_count(Ast *func, int expected, int got);
static AstList *op_resolve(Checker *c, Ast *proto, const char *name);
static void op_getter(Checker *c, Ast *func, Ast *proto, int isIndex);
static void op_setter(Checker *c, Ast *func, Ast *proto, int isIndex);
static void op_ander(Ast *func, int isIndex);
static void op_expect(Ast *func, int n);
static void op_bool_result(Ast *func, Checker *c);
static void check_operator(Checker *c, Ast *func, Ast *proto);
static void check_shader(Checker *c, Ast *func);
static void chk_funcdef(Checker *c, Ast *n);
static void chk_protocoldecl(Checker *c, Ast *n);
static int values_equal(Ast *ut, double a, double b);
static void chk_enumtype(Checker *c, Ast *n);
static void chk_type_args(Checker *c, Ast *n, const AstList *tps, const AstList *targs);
static void chk_typeref(Checker *c, Ast *n);
static void chk_arraytype(Checker *c, Ast *n);
static void chk_vardecl(Checker *c, Ast *n);
static Ast *chk_assign(Checker *c, Ast *n);
static Ast *chk_rmw(Checker *c, Ast *n);
static Ast *chk_deref(Checker *c, Ast *n);
static Ast *chk_makeptr(Checker *c, Ast *n);
static Ast *chk_makearrayref(Checker *c, Ast *n);
static void chk_requirebool(Checker *c, Ast *e);
static void chk_if(Checker *c, Ast *n);
static void chk_for(Checker *c, Ast *n);
static Ast *chk_comma(Checker *c, Ast *n);
static void chk_return(Checker *c, Ast *n);
static void chk_switch(Checker *c, Ast *n);
static Ast *chk_call(Checker *c, Ast *n);
static int is_type_statement(const Ast *st);
static void check_func_cb(void *cx, Ast *f);
static void check_enum_owners(Program *p);
static void check_program(Program *p);
static void post_check_passes(Program *p);
static void resolve_properties(Program *p);   /* wsl-props.h */


static int name_in(const char **v, int n, const char *s) {
  for (int i = 0; i < n; i++) if (strcmp(v[i], s) == 0) return 1;
  return 0;
}

/* ---- classification helpers ---- */
static int is_expression(const Ast *n) {
  switch (n->tag) {
  case A_ASSIGN: case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL:
  case A_NULLLITERAL: case A_BOOLLITERAL: case A_CALL: case A_DOT: case A_INDEX: case A_DEREF:
  case A_MAKEPTR: case A_MAKEARRAYREF: case A_LOGICALNOT: case A_LOGICAL: case A_RMW: case A_IDENTITY:
  case A_ENUMLITERAL: case A_VARIABLEREF: case A_COMMA: case A_CONVERTPTR:
    return 1;
  default:
    return 0;
  }
}

static int is_literal_expr(const Ast *n) {
  return n->tag == A_INTLITERAL || n->tag == A_UINTLITERAL || n->tag == A_FLOATLITERAL || n->tag == A_DOUBLELITERAL;
}

/* Value.isConstexpr */
static int is_constexpr(const Ast *n) {
  switch (n->tag) {
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL:
  case A_BOOLLITERAL: case A_ENUMLITERAL: case A_CONSTEXPRTYPEPARAM:
    return 1;
  case A_VARIABLEREF: return n->u.varRef.variable->tag == A_CONSTEXPRTYPEPARAM;
  case A_IDENTITY: return is_constexpr(n->u.identity.target);
  default: return 0;
  }
}

/* The type a variable declares (its `type` property in the JS). */
static Ast *var_type(const Ast *v) {
  switch (v->tag) {
  case A_FUNCPARAMETER: case A_CONSTEXPRTYPEPARAM: return v->u.named.type;
  case A_VARIABLEDECL: return v->u.varDecl.type;
  case A_ANONVAR: return v->type;
  default: return NULL;
  }
}

/* varIsLValue of a variable; Value.isLValue for every other expression, which the checker sets on
 * property accesses (PROP_CHK lv) and reads through identities. */
static int is_lvalue(const Ast *n) {
  switch (n->tag) {
  case A_VARIABLEREF: {
    AstTag t = n->u.varRef.variable->tag;
    return t == A_FUNCPARAMETER || t == A_VARIABLEDECL;
  }
  case A_DEREF: return 1;
  case A_IDENTITY: return is_lvalue(n->u.identity.target);
  case A_DOT: return n->u.dot.pc.lv == 1;
  case A_INDEX: return n->u.index.pc.lv == 1;
  default: return 0;
  }
}

/* addressSpace of an expression (undefined in the JS is NULL) */
static const char *addr_space(const Ast *n) {
  switch (n->tag) {
  case A_VARIABLEREF: return "thread";
  case A_DEREF: return n->u.deref.addrSpace;
  case A_IDENTITY: return addr_space(n->u.identity.target);
  case A_DOT: return n->u.dot.pc.addrSpace;
  case A_INDEX: return n->u.index.pc.addrSpace;
  default: return NULL;
  }
}

/* notLValueReasonString: "\n" + reason, or "" */
static const char *not_lvalue_string(const Ast *n) {
  const char *r = NULL;
  if (n->tag == A_DOT) r = n->u.dot.pc.lvReason;
  else if (n->tag == A_INDEX) r = n->u.index.pc.lvReason;
  return r ? cat(2, "\n", r) : "";
}

static Ast *unify_of(Ast *n) { return ut_unify_node(n); }

/* Node.equalsWithCommit: the unification context when a and b unify and verify, with its literals committed */
static int commit_ctx_nodes(UCtx *c);
static int equals_with_commit(Ast *a, Ast *b) {
  if (!b) chk_error("Null other");
  UCtx *c = ut_equals(a, b);
  if (!c) return 0;
  commit_ctx_nodes(c);
  return 1;
}

/* UnificationContext.commit: each literal type and NullType that is a type argument records its root */
static int commit_ctx_nodes(UCtx *c) {
  int total = 2 * c->nmap + c->nextra;
  Ast **nodes = arena_alloc(sizeof(Ast *) * (total ? total : 1));
  int n = uc_nodes(c, nodes);
  for (int i = 0; i < n; i++) {
    Ast *x = nodes[i];
    if (!is_lit_type_or_null(x)) continue;
    if (uc_in_tps(c, x)) continue;
    Ast *root = uc_find(c, x);
    if (x->tag == A_NULLTYPE) x->type = root;
    else x->type = rw_aw(root);
  }
  return 1;
}

/* Node.commit on a type: a context of that one node, verified, committed; returns its root */
static Ast *commit_node(Ast *t) {
  UCtx *c = uc_new(NULL, 0);
  uc_add_extra(c, t);
  if (!uc_verify_pub(c)) chk_error(UT_REASON);
  commit_ctx_nodes(c);
  return uc_find(c, t);
}

/* The value a literal expression holds in its selected type (GenericLiteral.valueForSelectedType). */
static double value_for_selected(const Ast *lit) {
  /* EnumLiteral.valueForSelectedType: the member's value's unify node */
  if (lit->tag == A_ENUMLITERAL) lit = unify_of((Ast *)lit->u.enumLit.member->u.nameOnly.value);
  const Ast *t = lit->type;
  if (is_lit_type(t)) t = t->type;
  Ast *nat = unify_of((Ast *)t->u.typeRef.type);
  LitKind lk = lit->tag == A_INTLITERAL ? LK_INT : lit->tag == A_UINTLITERAL ? LK_UINT
             : lit->tag == A_FLOATLITERAL ? LK_FLOAT : LK_DOUBLE;
  return nt_format_value(nt_kind(nat), lk, lit_value(lit));
}

/* ---- node makers (the JS constructors the checker calls) ---- */
static void uc_add_extra(UCtx *c, Ast *t) {
  if (c->nextra == c->capextra) {
    int cap = c->capextra ? 2 * c->capextra : 8;
    Ast **nv = arena_alloc(sizeof(Ast *) * cap);
    for (int i = 0; i < c->nextra; i++) nv[i] = c->extra[i];
    c->extra = nv;
    c->capextra = cap;
  }
  c->extra[c->nextra++] = t;
}

static Ast *mk_ptr_type(const Ast *src, const char *space, Ast *elem) {
  Ast *a = mk_from(A_PTRTYPE, src);
  a->u.ref.addressSpace = space;
  a->u.ref.elementType = elem;
  return a;
}

static Ast *mk_arrayref_type(const Ast *src, const char *space, Ast *elem) {
  Ast *a = mk_from(A_ARRAYREFTYPE, src);
  a->u.ref.addressSpace = space;
  a->u.ref.elementType = elem;
  return a;
}

/* An integer literal with a type: GenericLiteral.withType(origin, value, type) */
static Ast *mk_int_lit_typed(const Ast *src, AstTag tag, double value, Ast *type) {
  Ast *a = mk_from(tag, src);
  a->u.intLit.value = (long long)value;
  a->type = typeref_wrap(type);
  return a;
}

/* A node that is a list of the given things (the AstList of a call's arguments) */
static AstList alist(Ast **v, int n) {
  AstList l = {NULL, 0, 0};
  for (int i = 0; i < n; i++) list_push(&l, v[i]);
  return l;
}

static void alist_append(AstList *dst, const AstList *src) {
  for (int i = 0; i < src->n; i++) list_push(dst, src->v[i]);
}

/* the functions a name has in the program (JS Program.functions.get), NULL when none */
static AstList *funcs_named(Program *p, const char *name) {
  AstList *out = arena_alloc(sizeof(AstList));
  memset(out, 0, sizeof *out);
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if ((st->tag == A_FUNCDEF || st->tag == A_NATIVEFUNC) && strcmp(st->u.func.name, name) == 0)
      list_push(out, st);
  }
  return out->n ? out : NULL;
}

/* ProtocolDecl.signaturesByNameWithTypeVariable: each signature of that name with the protocol's type variable
 * replaced by tv; NULL when the protocol has no signature of the name */
static AstList *signatures_with_tv(Ast *pd, const char *name, Ast *tv) {
  AstList *out = NULL;
  for (int i = 0; i < pd->u.protocolDecl.signatures.n; i++) {
    Ast *sig = pd->u.protocolDecl.signatures.v[i];
    if (strcmp(sig->u.func.name, name) != 0) continue;
    if (!out) {
      out = arena_alloc(sizeof(AstList));
      memset(out, 0, sizeof *out);
    }
    Ast *olds[1] = {pd->u.protocolDecl.typeVariable};
    Ast *news[1] = {tv};
    list_push(out, subst_rw(sig, olds, news, 1));
  }
  return out;
}

/* ---- CallExpression (CallExpression.js) ---- */
/* the call node of a property access or a visited call: origin of src, the name, and no type arguments */
static Ast *mk_call_node(const Ast *src, const char *name) {
  Ast *c = mk_from(A_CALL, src);
  c->u.call.name = name;
  return c;
}

/* resolveToOverload: the call's function, its actual type arguments and its result type */
static Ast *resolve_to_overload(Ast *call, Overload *ov) {
  call->u.call.func = ov->func;
  AstList actual = {NULL, 0, 0};
  for (int i = 0; i < ov->ntargs; i++) {
    Ast *t = ov->targs[i];
    list_push(&actual, is_jstype(t) ? rw_aw(t) : t);
  }
  call->u.call.actualTA = actual;
  call->u.call.instTA = actual;   /* CallExpression.resolveToOverload: instantiatedActualTypeArguments = actualTypeArguments */
  Ast *r = subst_unif(ov->func->u.func.returnType, ov->func->u.func.typeParameters.v,
                      ov->func->u.func.typeParameters.n, ov->ctx);
  r = rw_aw(r);
  call->u.call.resultType = r;
  return r;
}

/* The message of an overload failure list: each candidate's failure, separated by newlines (failures.join) */
static const char *fails_joined(const char **f, int n) {
  const char *s = "";
  for (int i = 0; i < n; i++) s = cat(3, s, i ? "\n" : "", f[i]);
  return s;
}

/* CallExpression.resolve. call carries the argument types (argTypes) and the return type (cast). */
static Ast *call_resolve(Ast *call, AstList *possible, Ast **scope, int nscope, Ast **tv, int ntv) {
  const char *name = call->u.call.name;
  Ast *origin = call;
  if (!possible) wtype_error(origin_string(origin), cat(2, "Did not find any functions named ", name));
  Ast **targs = call->u.call.typeArguments.v;
  int ntargs = call->u.call.typeArguments.n;
  Ast **args = call->u.call.argTypes.v;
  int nargs = call->u.call.argTypes.n;
  Ast *ret = call->u.call.returnType;
  const char **fails = arena_alloc(sizeof(const char *) * 256);
  int nfails = 0;
  Overload ov = {0};
  int have = 0;
  for (int i = 0; i < nscope && !have; i++) {
    Ast *tp = scope[i];
    if (tp->tag != A_TYPEVARIABLE || !tp->u.typeVariable.protocol) continue;
    AstList *sigs = signatures_with_tv(ut_pdecl(tp->u.typeVariable.protocol), name, tp);
    if (!sigs) continue;
    ov = resolve_overload(sigs, targs, ntargs, args, nargs, ret, 0);
    if (ov.func) have = 1;
    else for (int k = 0; k < ov.nfails; k++) fails[nfails++] = ov.fails[k];
  }
  if (!have) {
    ov = resolve_overload(possible, targs, ntargs, args, nargs, ret, 0);
    if (!ov.func) {
      for (int k = 0; k < ov.nfails; k++) fails[nfails++] = ov.fails[k];
      const char *m = cat(2, "Did not find function named ", name);
      m = cat(2, m, " for call with ");
      if (ntargs) m = cat(4, m, "type arguments <", js_join(&call->u.call.typeArguments, ","), "> and ");
      m = cat(4, m, "argument types (", js_join(&call->u.call.argTypes, ","), ")");
      if (ret) m = cat(2, m, cat(2, " and return type ", js_tostr(ret)));
      if (nfails) m = cat(3, m, ", but considered:\n", fails_joined(fails, nfails));
      wtype_error(origin_string(origin), m);
    }
  }
  for (int i = 0; i < ntv; i++) {
    Ast *argT = tv[i];
    if (i >= ov.func->u.func.typeParameters.n) continue;
    Ast *tp = ov.func->u.func.typeParameters.v[i];
    if (tp->tag != A_CONSTEXPRTYPEPARAM) continue;
    if (!equals_with_commit(tp->u.named.type, argT))
      chk_error(cat(6, "At ", origin_string(origin), " constexpr type argument and parameter types not equal: argument = ",
                    js_tostr(argT), ", parameter = ", js_tostr(tp->u.named.type)));
  }
  for (int i = 0; i < nargs; i++) {
    Ast *argT = args[i];
    Ast *paramT = subst_unif(ov.func->u.func.parameters.v[i]->u.named.type, ov.func->u.func.typeParameters.v,
                             ov.func->u.func.typeParameters.n, ov.ctx);
    if (!equals_with_commit(argT, paramT))
      chk_error(cat(6, "At ", origin_string(origin),
                    " argument and parameter types not equal after type argument substitution: argument = ",
                    js_tostr(argT), ", parameter = ", js_tostr(paramT)));
  }
  return resolve_to_overload(call, &ov);
}

/* The result of CallExpression.resolve(origin, possible, scope, name, [], args, argTypes, null): the call
 * node is stored in *callOut; the result type is returned. A WTypeError propagates. */
typedef struct { Ast *node; AstList *possible; Ast **scope; int nscope; const char *name;
                 Ast **args; int nargs; Ast **argTypes; int nargTypes; Ast *callOut; Ast *res; } StaticResolve;
static void static_resolve_run(void *cx) {
  StaticResolve *k = cx;
  Ast *call = mk_call_node(k->node, k->name);
  call->u.call.argumentList = alist(k->args, k->nargs);
  call->u.call.hasOv = k->possible != NULL;
  call->u.call.ov = k->possible;
  for (int i = 0; i < k->nargTypes; i++) list_push(&call->u.call.argTypes, rw_aw(k->argTypes[i]));
  k->callOut = call;
  k->res = call_resolve(call, k->possible, k->scope, k->nscope, NULL, 0);
}

/* ---- property access (Checker._finishVisitingPropertyAccess) ---- */
static PropChk *prop_chk(Ast *n) { return n->tag == A_DOT ? &n->u.dot.pc : &n->u.index.pc; }
static AstList *prop_ovs(Ast *n, int which) {  /* 0 get, 1 and, 2 set */
  if (n->tag == A_DOT) return which == 0 ? (n->u.dot.hasGet ? n->u.dot.getOv : NULL)
                            : which == 2 ? (n->u.dot.hasSet ? n->u.dot.setOv : NULL)
                            : (n->u.dot.hasAnd ? n->u.dot.andOv : NULL);
  return which == 0 ? (n->u.index.hasGet ? n->u.index.getOv : NULL)
       : which == 2 ? (n->u.index.hasSet ? n->u.index.setOv : NULL)
       : (n->u.index.hasAnd ? n->u.index.andOv : NULL);
}
static const char *prop_name(Ast *n, int which) {
  if (n->tag == A_DOT) {
    const char *f = n->u.dot.fieldName;
    return which == 0 ? cat(2, "operator.", f) : which == 2 ? cat(3, "operator.", f, "=") : cat(2, "operator&.", f);
  }
  return which == 0 ? "operator[]" : which == 2 ? "operator[]=" : "operator&[]";
}

/* the access's type argument for the by-pointer overload: Type.argumentTypeForAndOverload */
static Ast *arg_type_for_and(Ast *origin, Ast *t) {
  switch (t->tag) {
  case A_PTRTYPE: wtype_error(origin_string(origin), "Pointer subscript is not valid");
  case A_ARRAYREFTYPE: return t;
  case A_ARRAYTYPE: return mk_arrayref_type(origin, "thread", t->u.arrayType.elementType);
  default: return mk_ptr_type(origin, "thread", t);
  }
  return NULL;
}

/* argumentForAndOverload(origin, value) */
static Ast *arg_for_and(Ast *origin, Ast *t, Ast *value) {
  switch (t->tag) {
  case A_PTRTYPE: wtype_error(origin_string(origin), "Pointer subscript is not valid");
  case A_ARRAYREFTYPE: return value;
  case A_ARRAYTYPE: {
    Ast *m = mk_from(A_MAKEARRAYREF, origin);
    m->u.lvalueRef.lValue = value;
    m->u.lvalueRef.numElements = t->u.arrayType.numElements;
    return m;
  }
  default: {
    Ast *m = mk_from(A_MAKEPTR, origin);
    m->u.lvalueRef.lValue = value;
    return m;
  }
  }
  return NULL;
}

/* returnTypeFromAndOverload(origin) */
static Ast *ret_from_and(Ast *origin, Ast *t) {
  if (t->tag == A_PTRTYPE) return t->u.ref.elementType;
  wtype_error(origin_string(origin), cat(2, "By-pointer overload returned non-pointer type: ", js_tostr(t)));
  return NULL;
}

/* the access being resolved: its base type (and the by-pointer argument type), its index arguments and scope */
typedef struct { Ast *node; Ast *baseType; Ast *typeForAnd; Ast **extra; Ast **extraT; int nextra; Ast **scope; int nscope; } PropCx;

static void prop_try_get(void *cx) {
  PropCx *k = cx;
  Ast *n = k->node;
  PropChk *pc = prop_chk(n);
  Ast **args = arena_alloc(sizeof(Ast *) * (k->nextra + 1));
  Ast **types = arena_alloc(sizeof(Ast *) * (k->nextra + 1));
  args[0] = n->tag == A_DOT ? n->u.dot.base : n->u.index.base;
  types[0] = k->baseType;
  for (int i = 0; i < k->nextra; i++) { args[i + 1] = k->extra[i]; types[i + 1] = k->extraT[i]; }
  StaticResolve s = {n, prop_ovs(n, 0), k->scope, k->nscope, prop_name(n, 0), args, k->nextra + 1, types, k->nextra + 1, NULL, NULL};
  static_resolve_run(&s);
  pc->callGet = s.callOut;
  pc->resGet = s.res;
}

static void prop_try_and(void *cx) {
  PropCx *k = cx;
  Ast *n = k->node;
  PropChk *pc = prop_chk(n);
  Ast *base = n->tag == A_DOT ? n->u.dot.base : n->u.index.base;
  Ast *baseForAnd = arg_for_and(n, k->baseType, base);
  Ast *typeForAnd = k->typeForAnd;
  Ast **args = arena_alloc(sizeof(Ast *) * (k->nextra + 1));
  Ast **types = arena_alloc(sizeof(Ast *) * (k->nextra + 1));
  args[0] = baseForAnd;
  types[0] = typeForAnd;
  for (int i = 0; i < k->nextra; i++) { args[i + 1] = k->extra[i]; types[i + 1] = k->extraT[i]; }
  StaticResolve s = {n, prop_ovs(n, 1), k->scope, k->nscope, prop_name(n, 1), args, k->nextra + 1, types, k->nextra + 1, NULL, NULL};
  static_resolve_run(&s);
  pc->callAnd = s.callOut;
  pc->resAnd = ret_from_and(n, unify_of(s.res));
}

static void prop_try_set(void *cx) {
  PropCx *k = cx;
  Ast *n = k->node;
  PropChk *pc = prop_chk(n);
  Ast *base = n->tag == A_DOT ? n->u.dot.base : n->u.index.base;
  Ast *resultType = pc->resGet ? pc->resGet : pc->resAnd;
  Ast **args = arena_alloc(sizeof(Ast *) * (k->nextra + 2));
  Ast **types = arena_alloc(sizeof(Ast *) * (k->nextra + 2));
  args[0] = base;
  types[0] = k->baseType;
  for (int i = 0; i < k->nextra; i++) { args[i + 1] = k->extra[i]; types[i + 1] = k->extraT[i]; }
  args[k->nextra + 1] = NULL;
  types[k->nextra + 1] = resultType;
  StaticResolve s = {n, prop_ovs(n, 2), k->scope, k->nscope, prop_name(n, 2), args, k->nextra + 2, types, k->nextra + 2, NULL, NULL};
  static_resolve_run(&s);
  pc->callSet = s.callOut;
  if (!ut_equals(s.res, k->baseType))
    wtype_error(origin_string(n), cat(4, "Result type of setter ", js_tostr(s.callOut->u.call.func), " is not the base type ", js_tostr(k->baseType)));
}

/* _finishVisitingPropertyAccess: base is the (unified) base type, extra the index arguments and their types */
static Ast *finish_prop(Checker *c, Ast *n, Ast *baseType, Ast **extra, Ast **extraT, int nextra) {
  PropChk *pc = prop_chk(n);
  baseType = rw_aw(baseType);
  pc->baseType = baseType;
  Ast *typeForAnd = arg_type_for_and(n, baseType);
  if (!typeForAnd) chk_error("Cannot get typeForAnd");
  AstList *tps = stmt_type_params(c);
  PropCx k = {n, baseType, typeForAnd, extra, extraT, nextra, tps ? tps->v : NULL, tps ? tps->n : 0};
  const char *raw = NULL;
  if (!try_wtype(prop_try_get, &k, &raw)) pc->errGet = raw;
  if (!try_wtype(prop_try_and, &k, &raw)) pc->errAnd = raw;
  if (!pc->resGet && !pc->resAnd)
    wtype_error(origin_string(n), cat(6, "Cannot resolve access; tried by-value:\n", pc->errGet, "\n",
                                       "and tried by-pointer:\n", pc->errAnd, ""));
  if (pc->resGet && pc->resAnd && !ut_equals(pc->resGet, pc->resAnd))
    wtype_error(origin_string(n), cat(5, "Result type resolved by-value (", js_tostr(pc->resGet),
                                       ") does not match result type resolved by-pointer (", js_tostr(pc->resAnd), ")"));
  if (!try_wtype(prop_try_set, &k, &raw)) pc->errSet = raw;
  if (!pc->callAnd && !pc->callSet) {
    pc->lv = 0;
    pc->lvReason = cat(6, "Have neither ander nor setter. Tried setter:\n", pc->errSet, "\n",
                       "and tried ander:\n", pc->errAnd, "");
  } else if (!is_lvalue(n->tag == A_DOT ? n->u.dot.base : n->u.index.base) && !(baseType->tag == A_ARRAYREFTYPE)) {
    pc->lv = 0;
    pc->lvReason = "Base of property access is neither a lvalue nor an array reference";
  } else {
    pc->lv = 1;
    Ast *base = n->tag == A_DOT ? n->u.dot.base : n->u.index.base;
    pc->addrSpace = is_lvalue(base) ? addr_space(base) : baseType->u.ref.addressSpace;
  }
  return pc->resGet ? pc->resGet : pc->resAnd;
}

/* _currentStatement.typeParameters */
static AstList *stmt_type_params(Checker *c) {
  Ast *st = c->stmt;
  if (!st) return NULL;
  if (st->tag == A_FUNCDEF || st->tag == A_NATIVEFUNC) return &st->u.func.typeParameters;
  return (AstList *)type_parameters_of(st);
}

/* ---- TypeVariableTracker ---- */
typedef struct { AstList *set; } Tvt;
static void tvt_consider(AstList *set, Ast *x) {
  if (!ut_is_unifiable(x)) return;
  for (int i = 0; i < set->n; i++) if (set->v[i] == x) return;
  list_push(set, x);
}
static void tvt_visit(Ast *n, AstList *set);
static void tvt_child(void *cx, Ast *c) { tvt_visit(c, (AstList *)cx); }
static void tvt_visit(Ast *n, AstList *set) {
  if (n->tag == A_TYPEREF) {
    if (n->u.typeRef.typeArguments.n) {
      for (int i = 0; i < n->u.typeRef.typeArguments.n; i++) tvt_visit(n->u.typeRef.typeArguments.v[i], set);
      return;
    }
    tvt_consider(set, n->u.typeRef.type);
    return;
  }
  if (n->tag == A_VARIABLEREF) { tvt_consider(set, n->u.varRef.variable); return; }
  visit_children(n, tvt_child, set);
}

/* ---- the visitor (Checker.js). chk returns the visit's value: a type for an expression, else NULL. ---- */
static Ast *chk(Checker *c, Ast *n);
static void chk_child(void *cx, Ast *child) { chk((Checker *)cx, child); }

static void trace_expr(Ast *n, Ast *r) {
  Buf *b = &CHK_TRACE;
  buf_str(b, "E ");
  buf_str(b, ast_class_name(n));
  buf_str(b, " ");
  buf_str(b, origin_tag(n));
  buf_str(b, " ");
  buf_str(b, r ? js_tostr(r) : "-");
  buf_str(b, "\n");
  if (n->tag == A_CALL) {
    buf_str(b, "C ");
    buf_str(b, n->u.call.name);
    buf_str(b, " ");
    buf_str(b, n->u.call.func ? js_decl(n->u.call.func) : "-");
    buf_str(b, " -> ");
    buf_str(b, n->u.call.resultType ? js_tostr(n->u.call.resultType) : "-");
    buf_str(b, "\n");
  }
}

static Ast *chk(Checker *c, Ast *n) {
  Ast *r = NULL;
  switch (n->tag) {
  case A_FUNCDEF: chk_funcdef(c, n); break;
  case A_NATIVEFUNC: break;
  case A_PROTOCOLFUNCDECL: visit_func_default(c, n); break;
  case A_TYPEREF: chk_typeref(c, n); break;
  case A_TYPEVARIABLE: case A_TYPEORVARIABLEREF: case A_PROTOCOLREF: case A_BREAK: case A_CONTINUE:
  case A_TRAP: break;
  case A_PTRTYPE: case A_ARRAYREFTYPE: chk(c, n->u.ref.elementType); break;
  case A_ARRAYTYPE: chk_arraytype(c, n); break;
  case A_CONSTEXPRTYPEPARAM: case A_FIELD: case A_FUNCPARAMETER: chk(c, n->u.named.type); break;
  case A_TYPEDEF: case A_STRUCTTYPE: case A_NATIVETYPE: case A_BLOCK: case A_SWITCHCASE:
    visit_children(n, chk_child, c);
    break;
  case A_ENUMTYPE: chk_enumtype(c, n); break;
  case A_ENUMMEMBER: if (n->u.nameOnly.value) chk(c, n->u.nameOnly.value); break;
  case A_PROTOCOLDECL: chk_protocoldecl(c, n); break;
  case A_VARIABLEDECL: chk_vardecl(c, n); break;
  case A_SWITCHSTATEMENT: chk_switch(c, n); break;
  case A_RETURN: chk_return(c, n); break;
  case A_IF: chk_if(c, n); break;
  case A_WHILE: chk_requirebool(c, n->u.loop.conditional); chk(c, n->u.loop.body); break;
  case A_DOWHILE: chk(c, n->u.loop.body); chk_requirebool(c, n->u.loop.conditional); break;
  case A_FOR: chk_for(c, n); break;
  case A_COMMA: r = chk_comma(c, n); break;
  case A_NULLLITERAL: r = n->type; break;
  case A_BOOLLITERAL: r = native_named(c->p, "bool"); break;
  case A_VARIABLEREF: {
    Ast *v = n->u.varRef.variable;
    r = var_type(v);
    if (!r) chk_error(cat(2, "Variable has no type: ", js_tostr(v)));
    break;
  }
  case A_CALL: r = chk_call(c, n); break;
  case A_DOT: r = finish_prop(c, n, unify_of(chk(c, n->u.dot.base)), NULL, NULL, 0); break;
  case A_INDEX: {
    Ast *arrayType = unify_of(chk(c, n->u.index.base));
    Ast *indexType = chk(c, n->u.index.index);
    r = finish_prop(c, n, arrayType, &n->u.index.index, &indexType, 1);
    break;
  }
  case A_DEREF: r = chk_deref(c, n); break;
  case A_MAKEPTR: r = chk_makeptr(c, n); break;
  case A_MAKEARRAYREF: r = chk_makearrayref(c, n); break;
  case A_CONVERTPTR: chk_error("Should not exist yet."); break;
  case A_LOGICALNOT: chk_requirebool(c, n->u.notExpr.operand); r = native_named(c->p, "bool"); break;
  case A_LOGICAL:
    chk_requirebool(c, n->u.logical.left);
    chk_requirebool(c, n->u.logical.right);
    r = native_named(c->p, "bool");
    break;
  case A_ASSIGN: r = chk_assign(c, n); break;
  case A_RMW: r = chk_rmw(c, n); break;
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL: r = n->type; break;
  case A_IDENTITY: r = chk(c, n->u.identity.target); break;
  case A_ENUMLITERAL: r = n->u.enumLit.member->u.nameOnly.target; break;
  case A_ANONVAR:
    if (!n->type) chk_error("Anonymous variable must know type before first appearance");
    break;
  default:
    chk_error(cat(2, "Checker: no visit for ", ast_class_name(n)));
  }
  if (is_expression(n)) trace_expr(n, r);
  return r;
}

static AstList EMPTY_LIST = {NULL, 0, 0};

/* Visitor.visitFunc */
static void visit_func_default(Checker *c, Ast *n) {
  chk(c, n->u.func.returnType);
  for (int i = 0; i < n->u.func.typeParameters.n; i++) chk(c, n->u.func.typeParameters.v[i]);
  for (int i = 0; i < n->u.func.parameters.n; i++) chk(c, n->u.func.parameters.v[i]);
}

static int text_is_identifier(const char *s) {
  int n = (int)strlen(s);
  if (!n || lex_is_digit(s[0]) || !lex_is_word(s[0])) return 0;
  for (int i = 0; i < n; i++) if (!lex_is_word(s[i])) return 0;
  return 1;
}

static int is_ptr_t(const Ast *t) { return t->tag == A_PTRTYPE; }
static int is_ref_t(const Ast *t) { return t->tag == A_PTRTYPE || t->tag == A_ARRAYREFTYPE; }

/* the message "Incorrect number of parameters for NAME (expected E, got G)" */
static void wrong_param_count(Ast *func, int expected, int got) {
  wtype_error(origin_string(func), cat(7, "Incorrect number of parameters for ", func->u.func.name, " (expected ",
                                         js_num(expected), ", got ", js_num(got), ")"));
}

/* the functions a name has for an operator check: the program's functions, or a protocol's signatures */
static AstList *op_resolve(Checker *c, Ast *proto, const char *name) {
  if (!proto) return funcs_named(c->p, name);
  AstList *out = NULL;
  for (int i = 0; i < proto->u.protocolDecl.signatures.n; i++) {
    Ast *sig = proto->u.protocolDecl.signatures.v[i];
    if (strcmp(sig->u.func.name, name) != 0) continue;
    if (!out) { out = arena_alloc(sizeof(AstList)); memset(out, 0, sizeof *out); }
    list_push(out, sig);
  }
  return out;
}

static void op_getter(Checker *c, Ast *func, Ast *proto, int isIndex) {
  int expected = isIndex ? 2 : 1;
  if (func->u.func.parameters.n != expected) wrong_param_count(func, expected, func->u.func.parameters.n);
  Ast *p0 = unify_of(func->u.func.parameters.v[0]->u.named.type);
  if (is_ptr_t(p0)) wtype_error(origin_string(func), cat(2, "Cannot have getter for pointer type: ", js_tostr(func->u.func.parameters.v[0]->u.named.type)));
  (void)c; (void)proto;
}

static void op_setter(Checker *c, Ast *func, Ast *proto, int isIndex) {
  int expected = isIndex ? 3 : 2;
  if (func->u.func.parameters.n != expected) wrong_param_count(func, expected, func->u.func.parameters.n);
  Ast *pt0 = func->u.func.parameters.v[0]->u.named.type;
  if (is_ptr_t(unify_of(pt0))) wtype_error(origin_string(func), cat(2, "Cannot have setter for pointer type: ", js_tostr(pt0)));
  if (!ut_equals(func->u.func.returnType, pt0))
    wtype_error(origin_string(func), cat(6, "First parameter type and return type of setter must match (parameter was ",
                                         js_tostr(pt0), " but return was ", js_tostr(func->u.func.returnType), ")", ""));
  Ast *valueType = func->u.func.parameters.v[expected - 1]->u.named.type;
  size_t len = strlen(func->u.func.name);
  char *getterName = arena_alloc(len);
  memcpy(getterName, func->u.func.name, len - 1);
  getterName[len - 1] = 0;
  AstList *getters = op_resolve(c, proto, getterName);
  if (!getters)
    wtype_error(origin_string(func), cat(4, "Every setter must have a matching getter, but did not find any function named ",
                                         getterName, " to match ", func->u.func.name));
  AstList argTypes = {NULL, 0, 0};
  for (int i = 0; i < expected - 1; i++) list_push(&argTypes, func->u.func.parameters.v[i]->u.named.type);
  Overload ov = resolve_overload(getters, NULL, 0, argTypes.v, argTypes.n, NULL, 0);
  if (!ov.func)
    wtype_error(origin_string(func), cat(5, "Did not find function named ", func->u.func.name, " with arguments ",
                                         js_join(&argTypes, ","),
                                         ov.nfails ? cat(2, "; tried:\n", fails_joined(ov.fails, ov.nfails)) : ""));
  Ast *resultType = subst_unif(ov.func->u.func.returnType, ov.func->u.func.typeParameters.v,
                               ov.func->u.func.typeParameters.n, ov.ctx);
  if (!ut_equals(resultType, valueType))
    wtype_error(origin_string(func), cat(7, "Setter and getter must agree on value type (getter at ",
                                         origin_string(ov.func), " says ", js_tostr(resultType), " while this setter says ",
                                         js_tostr(valueType), ")"));
}

static void op_ander(Ast *func, int isIndex) {
  int expected = isIndex ? 2 : 1;
  if (func->u.func.parameters.n != expected) wrong_param_count(func, expected, func->u.func.parameters.n);
  Ast *ret = func->u.func.returnType;
  if (!is_ptr_t(unify_of(ret))) wtype_error(origin_string(func), cat(2, "Return type of ander is not a pointer: ", js_tostr(ret)));
  Ast *pt0 = func->u.func.parameters.v[0]->u.named.type;
  if (!is_ref_t(unify_of(pt0))) wtype_error(origin_string(func), cat(2, "Parameter to ander is not a reference: ", js_tostr(pt0)));
}

static void op_expect(Ast *func, int n) {
  if (func->u.func.parameters.n != n) wrong_param_count(func, n, func->u.func.parameters.n);
}

static void op_bool_result(Ast *func, Checker *c) {
  if (!ut_equals(func->u.func.returnType, native_named(c->p, "bool")))
    wtype_error(origin_string(func), cat(4, "Return type of ", func->u.func.name, " must be bool but was ", js_tostr(func->u.func.returnType)));
}

/* Checker._checkOperatorOverload; proto is the protocol a signature belongs to, or NULL for a function */
static void check_operator(Checker *c, Ast *func, Ast *proto) {
  const char *name = func->u.func.name;
  if (text_is_identifier(name)) return;
  if (strncmp(name, "operator", 8) != 0) chk_error(cat(2, "Bad operator overload name: ", name));
  AstList set = {NULL, 0, 0};
  for (int i = 0; i < func->u.func.parameters.n; i++) tvt_visit(func->u.func.parameters.v[i]->u.named.type, &set);
  if (func->u.func.isCast) tvt_visit(func->u.func.returnType, &set);
  for (int i = 0; i < func->u.func.typeParameters.n; i++) {
    Ast *tp = func->u.func.typeParameters.v[i];
    int found = 0;
    for (int k = 0; k < set.n; k++) if (set.v[k] == tp) found = 1;
    if (!found)
      wtype_error(origin_string(tp), cat(5, "Type parameter ", js_tostr(tp), " to operator ", js_decl(func),
                                         " is not inferrable from value parameters"));
  }
  if (!strcmp(name, "operator cast")) return;
  if (!strcmp(name, "operator++") || !strcmp(name, "operator--")) {
    op_expect(func, 1);
    if (!ut_equals(func->u.func.parameters.v[0]->u.named.type, func->u.func.returnType))
      wtype_error(origin_string(func), cat(7, "Parameter type and return type must match for ", name, " (parameter is ",
                                           js_tostr(func->u.func.parameters.v[0]->u.named.type), " while return is ",
                                           js_tostr(func->u.func.returnType), ")"));
    return;
  }
  if (!strcmp(name, "operator+") || !strcmp(name, "operator-")) {
    int n = func->u.func.parameters.n;
    if (n != 1 && n != 2)
      wtype_error(origin_string(func), cat(5, "Incorrect number of parameters for ", name, " (expected 1 or 2, got ",
                                           js_num(n), ")"));
    return;
  }
  static const char *const TWO[] = {"operator*", "operator/", "operator%", "operator&", "operator|", "operator^",
                                    "operator<<", "operator>>", "operator==", "operator<", "operator<=", "operator>",
                                    "operator>=", NULL};
  for (int i = 0; TWO[i]; i++) {
    if (strcmp(name, TWO[i]) != 0) continue;
    op_expect(func, 2);
    if (i >= 8) op_bool_result(func, c);
    return;
  }
  if (!strcmp(name, "operator~")) { op_expect(func, 1); return; }
  if (!strcmp(name, "operator[]")) { op_getter(c, func, proto, 1); return; }
  if (!strcmp(name, "operator[]=")) { op_setter(c, func, proto, 1); return; }
  if (!strcmp(name, "operator&[]")) { op_ander(func, 1); return; }
  if (!strncmp(name, "operator.", 9)) {
    size_t n = strlen(name);
    if (name[n - 1] == '=') op_setter(c, func, proto, 0);
    else op_getter(c, func, proto, 0);
    return;
  }
  if (!strncmp(name, "operator&.", 10)) { op_ander(func, 0); return; }
  chk_error(cat(2, "Parser accepted unrecognized operator: ", name));
}

static void check_shader(Checker *c, Ast *func) {
  if (func->u.func.typeParameters.n != 0)
    wtype_error(origin_string(func), cat(3, "Entry point ", func->u.func.name, " must not have type arguments."));
  if (!strcmp(func->u.func.shaderType, "vertex")) {
    if (name_in(c->vertex, c->nvertex, func->u.func.name))
      wtype_error(origin_string(func), cat(2, "Duplicate vertex entry point name ", func->u.func.name));
    c->vertex[c->nvertex++] = func->u.func.name;
  } else if (!strcmp(func->u.func.shaderType, "fragment")) {
    if (name_in(c->fragment, c->nfragment, func->u.func.name))
      wtype_error(origin_string(func), cat(2, "Duplicate fragment entry point name ", func->u.func.name));
    c->fragment[c->nfragment++] = func->u.func.name;
  }
}

static void chk_funcdef(Checker *c, Ast *n) {
  if (n->u.func.shaderType) check_shader(c, n);
  check_operator(c, n, NULL);
  chk(c, n->u.func.body);
}

static void chk_protocoldecl(Checker *c, Ast *n) {
  for (int i = 0; i < n->u.protocolDecl.signatures.n; i++) {
    Ast *sig = n->u.protocolDecl.signatures.v[i];
    AstList set = {NULL, 0, 0};
    for (int k = 0; k < sig->u.func.parameters.n; k++) tvt_visit(sig->u.func.parameters.v[k]->u.named.type, &set);
    if (sig->u.func.isCast) tvt_visit(sig->u.func.returnType, &set);
    for (int k = 0; k < sig->u.func.typeParameters.n; k++) {
      int found = 0;
      for (int j = 0; j < set.n; j++) if (set.v[j] == sig->u.func.typeParameters.v[k]) found = 1;
      if (!found) wsl_throw(0, "TypeError", "Class constructor WTypeError cannot be invoked without 'new'");
    }
    int found = 0;
    for (int j = 0; j < set.n; j++) if (set.v[j] == n->u.protocolDecl.typeVariable) found = 1;
    if (!found)
      wtype_error(origin_string(sig), cat(4, "Protocol's type variable (", n->u.protocolDecl.name,
                                         ") not mentioned in signature: ", js_tostr(sig)));
    check_operator(c, sig, n);
  }
}

/* values_equal of a native int or an enum over its base type */
static int values_equal(Ast *ut, double a, double b) {
  if (ut->tag == A_ENUMTYPE) return values_equal(unify_of(ut->u.enumType.baseType), a, b);
  return nt_values_equal(nt_kind(ut), a, b);
}

static void chk_enumtype(Checker *c, Ast *n) {
  Ast *base = n->u.enumType.baseType;
  chk(c, base);
  Ast *bu = unify_of(base);
  if (!ty_is_int(bu)) wtype_error(origin_string(n), cat(2, "Base type of enum is not an integer: ", js_tostr(base)));
  for (int i = 0; i < n->u.enumType.members.n; i++) {
    Ast *m = n->u.enumType.members.v[i];
    if (!m->u.nameOnly.value) continue;
    Ast *mt = chk(c, m->u.nameOnly.value);
    if (!equals_with_commit(bu, mt))
      wtype_error(origin_string(m), cat(6, "Type of enum member undefined does not patch enum base type (member type is ",
                                         js_tostr(mt), ", enum base type is ", js_tostr(base), ")", ""));
  }
  NtKind k = nt_kind(bu);
  double next = nt_default_value(k);
  for (int i = 0; i < n->u.enumType.members.n; i++) {
    Ast *m = n->u.enumType.members.v[i];
    if (m->u.nameOnly.value) {
      next = nt_successor(k, value_for_selected(unify_of(m->u.nameOnly.value)));
      continue;
    }
    m->u.nameOnly.value = mk_int_lit_typed(m, A_INTLITERAL, nt_create_literal(k, next), bu);
    next = nt_successor(k, next);
  }
  AstList *ms = &n->u.enumType.members;
  for (int i = 0; i < ms->n; i++) {
    Ast *mi = ms->v[i];
    for (int j = i + 1; j < ms->n; j++) {
      Ast *mj = ms->v[j];
      if (values_equal(bu, value_for_selected(unify_of(mi->u.nameOnly.value)),
                       value_for_selected(unify_of(mj->u.nameOnly.value))))
        wtype_error(origin_string(mj), cat(9, "Duplicate enum member value (", mi->u.nameOnly.name, " has ",
                                           js_tostr(mi->u.nameOnly.value), " while ", mj->u.nameOnly.name, " has ",
                                           js_tostr(mj->u.nameOnly.value), ")"));
    }
  }
  int foundZero = 0;
  for (int i = 0; i < ms->n; i++)
    if (values_equal(bu, value_for_selected(unify_of(ms->v[i]->u.nameOnly.value)), nt_default_value(k))) { foundZero = 1; break; }
  if (!foundZero) wtype_error(origin_string(n), "Enum does not have a member with the value zero");
}

/* Checker._checkTypeArguments */
static void chk_type_args(Checker *c, Ast *n, const AstList *tps, const AstList *targs) {
  for (int i = 0; i < tps->n; i++) {
    Ast *ta = targs->v[i];
    Ast *tp = tps->v[i];
    int argIsType = is_jstype(ta);
    Ast *res = chk(c, ta);
    if (argIsType) {
      Ast *proto = tp->tag == A_TYPEVARIABLE ? tp->u.typeVariable.protocol : NULL;
      if (!ut_inherits(ta, proto))
        wtype_error(origin_string(n), cat(2, "Type argument does not inherit protocol: ", UT_REASON));
    } else {
      if (!res) wsl_throw(0, "TypeError", "Cannot read properties of undefined (reading 'equalsWithCommit')");
      if (!equals_with_commit(res, tp->tag == A_CONSTEXPRTYPEPARAM ? tp->u.named.type : NULL))
        wtype_error(origin_string(n), "Wrong type for constexpr");
    }
  }
}

static void chk_typeref(Checker *c, Ast *n) {
  Ast *t = n->u.typeRef.type;
  if (!t) chk_error(cat(4, "Type reference without a type in checker: ", js_tostr(n), " at ", origin_string(n)));
  if (t->tag != A_STRUCTTYPE) chk(c, t);
  const AstList *tps = type_parameters_of(t);
  chk_type_args(c, n, tps ? tps : &EMPTY_LIST, &n->u.typeRef.typeArguments);
}

static void chk_arraytype(Checker *c, Ast *n) {
  chk(c, n->u.arrayType.elementType);
  Ast *num = n->u.arrayType.numElements;
  if (!is_constexpr(num)) wtype_error(origin_string(n), "Array length must be constexpr");
  Ast *t = chk(c, num);
  if (!equals_with_commit(t, native_named(c->p, "uint32")))
    wtype_error(origin_string(n), "Array length must be a uint32");
}

static void chk_vardecl(Checker *c, Ast *n) {
  chk(c, n->u.varDecl.type);
  if (n->u.varDecl.initializer) {
    Ast *lhs = n->u.varDecl.type;
    Ast *rhs = chk(c, n->u.varDecl.initializer);
    if (!equals_with_commit(lhs, rhs))
      wtype_error(origin_string(n), cat(4, "Type mismatch in variable initialization: ", js_tostr(lhs), " versus ", js_tostr(rhs)));
  }
}

static Ast *chk_assign(Checker *c, Ast *n) {
  Ast *lhs = n->u.assign.lhs;
  Ast *lhsT = chk(c, lhs);
  if (!is_lvalue(lhs)) wtype_error(origin_string(n), cat(3, "LHS of assignment is not an LValue: ", js_tostr(lhs), not_lvalue_string(lhs)));
  Ast *rhsT = chk(c, n->u.assign.rhs);
  if (!equals_with_commit(lhsT, rhsT))
    wtype_error(origin_string(n), cat(4, "Type mismatch in assignment: ", js_tostr(lhsT), " versus ", js_tostr(rhsT)));
  n->type = lhsT;
  return lhsT;
}

static Ast *chk_rmw(Checker *c, Ast *n) {
  Ast *lv = n->u.rmw.lValue;
  Ast *lhsT = chk(c, lv);
  if (!is_lvalue(lv))
    wtype_error(origin_string(n), cat(3, "LHS of read-modify-write is not an LValue: ", js_tostr(lv), not_lvalue_string(lv)));
  n->u.rmw.oldVar->type = lhsT;
  n->u.rmw.newVar->type = lhsT;
  chk(c, n->u.rmw.oldVar);
  chk(c, n->u.rmw.newVar);
  Ast *newT = chk(c, n->u.rmw.newValueExp);
  /* checker: the mismatch is `return new WTypeError`, a value the JS never reaches in a test program */
  if (!equals_with_commit(lhsT, newT)) chk_error("read-modify-write type mismatch (returned, not thrown, in JS)");
  return chk(c, n->u.rmw.resultExp);
}

static Ast *chk_deref(Checker *c, Ast *n) {
  Ast *t = unify_of(chk(c, n->u.deref.ptr));
  if (!is_ptr_t(t)) wtype_error(origin_string(n), cat(2, "Type passed to dereference is not a pointer: ", js_tostr(t)));
  n->type = t->u.ref.elementType;
  n->u.deref.addrSpace = t->u.ref.addressSpace;
  if (!n->u.deref.addrSpace) chk_error(cat(2, "Null address space in type: ", js_tostr(t)));
  return n->type;
}

static Ast *chk_makeptr(Checker *c, Ast *n) {
  Ast *lv = n->u.lvalueRef.lValue;
  Ast *elem = unify_of(chk(c, lv));
  if (!is_lvalue(lv)) wtype_error(origin_string(n), cat(3, "Operand to & is not an LValue: ", js_tostr(lv), not_lvalue_string(lv)));
  return mk_ptr_type(n, addr_space(lv), elem);
}

static Ast *chk_makearrayref(Checker *c, Ast *n) {
  Ast *lv = n->u.lvalueRef.lValue;
  Ast *elem = unify_of(chk(c, lv));
  if (is_ptr_t(elem)) {
    Ast *conv = mk_from(A_CONVERTPTR, n);
    conv->u.lvalueRef.lValue = lv;
    ast_become(n, conv);
    return mk_arrayref_type(n, elem->u.ref.addressSpace, elem->u.ref.elementType);
  }
  if (!is_lvalue(lv)) wtype_error(origin_string(n), cat(3, "Operand to @ is not an LValue: ", js_tostr(lv), not_lvalue_string(lv)));
  if (elem->tag == A_ARRAYTYPE) {
    n->u.lvalueRef.numElements = elem->u.arrayType.numElements;
    elem = elem->u.arrayType.elementType;
  } else {
    n->u.lvalueRef.numElements = mk_int_lit_typed(n, A_UINTLITERAL, 1, native_named(c->p, "uint32"));
  }
  return mk_arrayref_type(n, addr_space(lv), elem);
}

static void chk_requirebool(Checker *c, Ast *e) {
  Ast *t = chk(c, e);
  if (!t) chk_error(cat(2, "Expression has no type, but should be bool: ", js_tostr(e)));
  if (!ut_equals(t, native_named(c->p, "bool")))
    wtype_error_undef(cat(2, "Expression isn't a bool: ", js_tostr(e)));
}

static void chk_if(Checker *c, Ast *n) {
  chk_requirebool(c, n->u.ifStmt.conditional);
  chk(c, n->u.ifStmt.body);
  if (n->u.ifStmt.elseBody) chk(c, n->u.ifStmt.elseBody);
}

static void chk_for(Checker *c, Ast *n) {
  if (n->u.forLoop.initialization) chk(c, n->u.forLoop.initialization);
  if (n->u.forLoop.condition) chk_requirebool(c, n->u.forLoop.condition);
  if (n->u.forLoop.increment) chk(c, n->u.forLoop.increment);
  chk(c, n->u.forLoop.body);
}

static Ast *chk_comma(Checker *c, Ast *n) {
  Ast *r = NULL;
  for (int i = 0; i < n->u.comma.list.n; i++) r = chk(c, n->u.comma.list.v[i]);
  return r;
}

static void chk_return(Checker *c, Ast *n) {
  Ast *func = n->u.ret.func;
  if (n->u.ret.value) {
    Ast *rt = chk(c, n->u.ret.value);
    if (!rt) chk_error(cat(2, "Null result type from ", js_tostr(n->u.ret.value)));
    if (!equals_with_commit(func->u.func.returnType, rt))
      wtype_error(origin_string(n), cat(4, "Trying to return ", js_tostr(rt), " in a function that returns ",
                                         js_tostr(func->u.func.returnType)));
    return;
  }
  if (!equals_with_commit(func->u.func.returnType, native_named(c->p, "void")))
    wtype_error(origin_string(n), "Non-void function must return a value");
}

/* values of a native int kind, in allValues order, until the switch finds one it does not list */
static void chk_switch(Checker *c, Ast *n) {
  Ast *t = chk(c, n->u.switchStmt.value);
  if (!t) wsl_throw(0, "TypeError", "Cannot read properties of undefined (reading 'commit')");
  Ast *type = commit_node(t);
  Ast *ut = unify_of(type);
  if (!ty_is_int(ut) && ut->tag != A_ENUMTYPE)
    wtype_error(origin_string(n), cat(2, "Cannot switch on non-integer/non-enum type: ", js_tostr(type)));
  n->type = type;
  int hasDefault = 0;
  AstList *cases = &n->u.switchStmt.cases;
  for (int i = 0; i < cases->n; i++) {
    Ast *sc = cases->v[i];
    chk(c, sc->u.switchCase.body);
    if (!sc->u.switchCase.value) { hasDefault = 1; continue; }
    Ast *val = sc->u.switchCase.value;
    if (!is_constexpr(val)) wtype_error(origin_string(sc), cat(2, "Switch case not constexpr: ", js_tostr(val)));
    Ast *caseType = chk(c, val);
    if (!equals_with_commit(type, caseType))
      wtype_error(origin_string(sc), cat(6, "Switch case type does not match switch value type (case type is ",
                                         js_tostr(caseType), " but switch value type is ", js_tostr(type), ")", ""));
  }
  for (int i = 0; i < cases->n; i++) {
    Ast *first = cases->v[i];
    for (int j = i + 1; j < cases->n; j++) {
      Ast *second = cases->v[j];
      int fd = !first->u.switchCase.value, sd = !second->u.switchCase.value;
      if (fd != sd) continue;
      if (fd) wtype_error(origin_string(second), "Duplicate default case in switch statement");
      if (values_equal(ut, value_for_selected(unify_of(first->u.switchCase.value)),
                       value_for_selected(unify_of(second->u.switchCase.value))))
        wtype_error(origin_string(second), cat(2, "Duplicate case in switch statement for value ",
                                               js_num(value_for_selected(unify_of(first->u.switchCase.value)))));
    }
  }
  if (hasDefault) return;
  /* allValues: the enum members, or every value of the int kind; the first one no case lists is the error */
  double *seen = arena_alloc(sizeof(double) * (cases->n ? cases->n : 1));
  int nseen = 0;
  for (int i = 0; i < cases->n; i++) seen[nseen++] = value_for_selected(unify_of(cases->v[i]->u.switchCase.value));
#define CHK_SEEN(v) ({ int _f = 0; for (int _k = 0; _k < nseen; _k++) if (seen[_k] == (v)) _f = 1; _f; })
  if (ut->tag == A_ENUMTYPE) {
    AstList *ms = &ut->u.enumType.members;
    for (int i = 0; i < ms->n; i++) {
      double v = value_for_selected(unify_of(ms->v[i]->u.nameOnly.value));
      if (!CHK_SEEN(v))
        wtype_error(origin_string(n), cat(2, "Value not handled by switch statement: ", ms->v[i]->u.nameOnly.name));
    }
  } else {
    NtKind k = nt_kind(ut);
    for (long long i = 0; i <= 0xffffffffLL; i++) {
      double v = k == NT_INT32 ? (double)(int32_t)(uint32_t)i : k == NT_UINT8 ? (double)(i & 0xff) : (double)i;
      if (k == NT_UINT8 && i > 0xff) break;
      if (!CHK_SEEN(v))
        wtype_error(origin_string(n), cat(2, "Value not handled by switch statement: ", js_num(v)));
    }
  }
#undef CHK_SEEN
}

static Ast *chk_call(Checker *c, Ast *n) {
  int nt = n->u.call.typeArguments.n;
  Ast **tv = arena_alloc(sizeof(Ast *) * (nt ? nt : 1));
  for (int i = 0; i < nt; i++) tv[i] = chk(c, n->u.call.typeArguments.v[i]);
  n->u.call.argTypes.n = 0;
  for (int i = 0; i < n->u.call.argumentList.n; i++) {
    Ast *a = n->u.call.argumentList.v[i];
    Ast *r = chk(c, a);
    if (!r) chk_error(cat(2, "visitor returned null for ", js_tostr(a)));
    list_push(&n->u.call.argTypes, rw_aw(r));
  }
  if (n->u.call.returnType) chk(c, n->u.call.returnType);
  AstList *scope = stmt_type_params(c);
  return call_resolve(n, n->u.call.hasOv ? n->u.call.ov : NULL, scope ? scope->v : NULL, scope ? scope->n : 0, tv, nt);
}

/* ---- Checker.visitProgram ---- */
static int is_type_statement(const Ast *st) {
  return st->tag == A_STRUCTTYPE || st->tag == A_ENUMTYPE || st->tag == A_NATIVETYPE || st->tag == A_TYPEDEF;
}
static void check_func_cb(void *cx, Ast *f) {
  Checker *c = cx;
  c->stmt = f;
  chk(c, f);
}
static void check_func_sig_cb(void *cx, Ast *f) { visit_func_default((Checker *)cx, f); }

/* EnumType.add gives each member its enum; the checker and the literals read it through the member */
static void check_enum_owners(Program *p) {
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (st->tag != A_ENUMTYPE) continue;
    for (int k = 0; k < st->u.enumType.members.n; k++) st->u.enumType.members.v[k]->u.nameOnly.target = st;
  }
}

/* Check.js: check(program) */
/* ---- LiteralTypeChecker.js: every null and literal type must have its type (an invariant, so Error) ---- */
static void littype_visit(Ast *n);
static void littype_child(void *cx, Ast *c) { (void)cx; littype_visit(c); }
static void littype_visit(Ast *n) {
  switch (n->tag) {
  case A_NULLTYPE:
    if (!n->type) chk_error(cat(4, "Null at ", origin_string(n), " does not have type", ""));
    return;
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
    if (!n->type) chk_error(cat(4, js_tostr(n), " at ", origin_string(n), " does not have type"));
    return;
  default:
    visit_children(n, littype_child, NULL);
  }
}

/* ---- LoopChecker.js: break, continue and nested functions inside loops ---- */
typedef struct { int loopDepth, switchDepth; } LoopChk;
static void loopchk_visit(LoopChk *s, Ast *n);
static void loopchk_child(void *cx, Ast *c) { loopchk_visit((LoopChk *)cx, c); }
static void loopchk_body(LoopChk *s, Ast *n, Ast *body) {
  s->loopDepth++;
  loopchk_visit(s, body);
  if (s->loopDepth == 0) wtype_error(origin_string(n), "The number of nested loops is negative!");
  s->loopDepth--;
}
static void loopchk_visit(LoopChk *s, Ast *n) {
  switch (n->tag) {
  case A_FUNCDEF:
    if (s->loopDepth != 0) wtype_error(origin_string(n), "LoopChecker does not understand nested functions.");
    visit_children(n, loopchk_child, s);
    return;
  case A_WHILE:
    loopchk_visit(s, n->u.loop.conditional);
    loopchk_body(s, n, n->u.loop.body);
    return;
  case A_DOWHILE:
    loopchk_body(s, n, n->u.loop.body);
    loopchk_visit(s, n->u.loop.conditional);
    return;
  case A_FOR:
    if (n->u.forLoop.initialization) loopchk_visit(s, n->u.forLoop.initialization);
    if (n->u.forLoop.condition) loopchk_visit(s, n->u.forLoop.condition);
    if (n->u.forLoop.increment) loopchk_visit(s, n->u.forLoop.increment);
    loopchk_body(s, n, n->u.forLoop.body);
    return;
  case A_SWITCHSTATEMENT:
    loopchk_visit(s, n->u.switchStmt.value);
    s->switchDepth++;
    for (int i = 0; i < n->u.switchStmt.cases.n; i++) loopchk_visit(s, n->u.switchStmt.cases.v[i]);
    s->switchDepth--;
    return;
  case A_BREAK:
    if (s->loopDepth == 0 && s->switchDepth == 0)
      wtype_error(origin_string(n), "Break statement without enclosing loop or switch!");
    return;
  case A_CONTINUE:
    if (s->loopDepth == 0) wtype_error(origin_string(n), "Continue statement without enclosing loop!");
    return;
  default:
    visit_children(n, loopchk_child, s);
  }
}

/* ---- ReturnChecker.js: which statements definitely return. 0 is undefined in JS (a visit that returned
 * nothing), which the merges and the loop cases treat as "no style". ---- */
enum { RS_NONE = 0, RS_RET, RS_NORET, RS_HAS };
static Program *RR_PROG;

static int rs_merge(int a, int b) {
  if (!a) return b;
  if (!b) return a;
  switch (a) {
  case RS_RET: case RS_NORET: return a == b ? a : RS_HAS;
  case RS_HAS: return RS_HAS;
  default: chk_error("Bad return style");
  }
  return RS_NONE;
}

static int rr_visit(Ast *n);
static void rr_ignore_child(void *cx, Ast *c) { (void)cx; rr_visit(c); }
static void rr_default_walk(Ast *n) {
  /* the Visitor default: visit the children, ignoring what they return */
  visit_children(n, rr_ignore_child, NULL);
}

static int rr_is_bool_cast_true(const Ast *n) {
  if (!(n->tag == A_CALL && n->u.call.isCast)) return 0;
  if (!n->u.call.returnType || n->u.call.returnType->tag != A_TYPEREF) return 0;
  if (!ut_equals(n->u.call.returnType, native_named(RR_PROG, "bool"))) return 0;
  return n->u.call.argumentList.n == 1 && n->u.call.argumentList.v[0]->tag == A_BOOLLITERAL
         && n->u.call.argumentList.v[0]->u.boolLit.value;
}

static int rr_visit(Ast *n) {
  switch (n->tag) {
  case A_FUNCDEF: {
    if (ut_equals(n->u.func.returnType, native_named(RR_PROG, "void"))) return RS_NONE;
    int bv = rr_visit(n->u.func.body);
    if (bv == RS_NORET || bv == RS_HAS) wtype_error(origin_string(n), "Function does not return");
    return RS_NONE;
  }
  case A_BLOCK:
    for (int i = 0; i < n->u.block.statements.n; i++) {
      int st = rr_visit(n->u.block.statements.v[i]);
      if (st == RS_RET) return RS_RET;
      if (st == RS_NORET) return RS_NORET;
    }
    return RS_HAS;
  case A_IF: {
    if (!n->u.ifStmt.elseBody) return RS_HAS;
    int b = rr_visit(n->u.ifStmt.body);
    int e = rr_visit(n->u.ifStmt.elseBody);
    return rs_merge(b, e);
  }
  case A_WHILE: {
    int b = rr_visit(n->u.loop.body);
    if (n->u.loop.conditional->tag == A_CALL && rr_is_bool_cast_true(n->u.loop.conditional) && b == RS_RET)
      return RS_RET;
    return RS_HAS;
  }
  case A_DOWHILE:
    rr_visit(n->u.loop.body);   /* the JS switch falls through, so the result is always HasntReturnedYet */
    return RS_HAS;
  case A_FOR: {
    int b = rr_visit(n->u.forLoop.body);
    if (!n->u.forLoop.condition || rr_is_bool_cast_true(n->u.forLoop.condition)) {
      if (b == RS_RET) return RS_RET;
      if (b == RS_NORET || b == RS_HAS) return RS_HAS;
    }
    return RS_NONE;
  }
  case A_SWITCHSTATEMENT: {
    int rs = RS_NONE;
    for (int i = 0; i < n->u.switchStmt.cases.n; i++) {
      int bs = rr_visit(n->u.switchStmt.cases.v[i]->u.switchCase.body);
      if (bs == RS_NORET) bs = RS_HAS;   /* the FIXME: a break does not end the function */
      rs = rs_merge(rs, bs);
    }
    return rs;
  }
  case A_RETURN: case A_TRAP: return RS_RET;
  case A_BREAK: case A_CONTINUE: return RS_NORET;
  default:
    rr_default_walk(n);
    return RS_NONE;
  }
}

/* ---- UnreachableCodeChecker.js: a statement after one that returns or breaks ---- */
static void unreach_visit(Ast *n);
static void unreach_child(void *cx, Ast *c) { (void)cx; unreach_visit(c); }
static void unreach_visit(Ast *n) {
  visit_children(n, unreach_child, NULL);   /* super.visitBlock, and the default walk */
  if (n->tag != A_BLOCK) return;
  for (int i = 0; i + 1 < n->u.block.statements.n; i++) {
    int st = rr_visit(n->u.block.statements.v[i]);
    if (st == RS_RET || st == RS_NORET) wtype_error(origin_string(n->u.block.statements.v[i + 1]), "Unreachable code");
  }
}

/* ---- RecursionChecker.js: a function that calls itself, through the functions being visited ---- */
static Ast **RR_STACK;
static int RR_NSTACK, RR_CAP;
static void rec_visit(Ast *n);
static void rec_child(void *cx, Ast *c) { (void)cx; rec_visit(c); }
static void rec_visit(Ast *n) {
  switch (n->tag) {
  case A_FUNCDEF:
    for (int i = 0; i < RR_NSTACK; i++)
      if (RR_STACK[i] == n) wtype_error(origin_string(n), "Recursive Func");
    if (RR_NSTACK == RR_CAP) {
      int cap = RR_CAP ? 2 * RR_CAP : 64;
      Ast **nv = arena_alloc(sizeof(Ast *) * cap);
      for (int i = 0; i < RR_NSTACK; i++) nv[i] = RR_STACK[i];
      RR_STACK = nv;
      RR_CAP = cap;
    }
    RR_STACK[RR_NSTACK++] = n;
    visit_children(n, rec_child, NULL);
    RR_NSTACK--;
    return;
  case A_CALL:   /* visitCallExpression visits only the callee, not the arguments */
    if (!n->u.call.func) wsl_throw(0, "TypeError", "Cannot read properties of undefined (reading 'visit')");
    rec_visit(n->u.call.func);
    return;
  default:
    visit_children(n, rec_child, NULL);
  }
}

/* ---- ExpressionFinder.js: the expressions and types the wrapped check looks at ---- */
typedef void (*ExprCb)(Ast *expr);
static ExprCb FIN_CB;
static void fin_visit(Ast *n);
static void fin_child(void *cx, Ast *c) { (void)cx; fin_visit(c); }
static void fin_func(Ast *n) {   /* ExpressionFinder.visitFunc: no body, the body is visited by visitFuncDef */
  FIN_CB(n->u.func.returnType);
  for (int i = 0; i < n->u.func.typeParameters.n; i++) fin_visit(n->u.func.typeParameters.v[i]);
  for (int i = 0; i < n->u.func.parameters.n; i++) fin_visit(n->u.func.parameters.v[i]);
}
static void fin_visit(Ast *n) {
  switch (n->tag) {
  case A_FUNCDEF: fin_func(n); fin_visit(n->u.func.body); return;
  case A_NATIVEFUNC: case A_PROTOCOLFUNCDECL: fin_func(n); return;
  case A_FUNCPARAMETER: case A_CONSTEXPRTYPEPARAM: FIN_CB(n->u.named.type); return;
  case A_ASSIGN: case A_RMW: case A_IDENTITY: case A_CALL: FIN_CB(n); return;
  case A_RETURN: if (n->u.ret.value) FIN_CB(n->u.ret.value); return;
  case A_WHILE: FIN_CB(n->u.loop.conditional); fin_visit(n->u.loop.body); return;
  case A_DOWHILE: fin_visit(n->u.loop.body); FIN_CB(n->u.loop.conditional); return;
  case A_IF:
    FIN_CB(n->u.ifStmt.conditional);
    fin_visit(n->u.ifStmt.body);
    if (n->u.ifStmt.elseBody) fin_visit(n->u.ifStmt.elseBody);
    return;
  case A_FOR:
    if (n->u.forLoop.initialization) fin_visit(n->u.forLoop.initialization);
    if (n->u.forLoop.condition) FIN_CB(n->u.forLoop.condition);
    if (n->u.forLoop.increment) FIN_CB(n->u.forLoop.increment);
    fin_visit(n->u.forLoop.body);
    return;
  case A_VARIABLEDECL:
    FIN_CB(n->u.varDecl.type);
    if (n->u.varDecl.initializer) FIN_CB(n->u.varDecl.initializer);
    return;
  default:
    visit_children(n, fin_child, NULL);
  }
}

/* ---- WrapChecker.js: a wrapped expression holds no declarations, parameters or type variables ---- */
static Ast *WRAP_START;
static const char *origin_or_null(const Ast *n) { return n->origin < 0 ? "<null origin>" : origin_string(n); }
static void wrap_visit(Ast *n);
static void wrap_child(void *cx, Ast *c) { (void)cx; wrap_visit(c); }
static void wrap_visit(Ast *n) {
  switch (n->tag) {
  case A_VARIABLEREF: case A_TYPEREF: return;
  case A_CONSTEXPRTYPEPARAM: case A_FUNCPARAMETER: case A_VARIABLEDECL: case A_STRUCTTYPE:
  case A_NATIVETYPE: case A_TYPEVARIABLE:
    /* the JS reads node.origin.originString, and the start node's origin is a token, so its text is undefined */
    chk_error(cat(10, "Found unwrapped ", ast_class_name(n), " at ", origin_or_null(n), ": ", js_tostr(n),
                  "\nWhile visiting ", ast_class_name(WRAP_START), " at undefined: ", js_tostr(WRAP_START)));
  default:
    visit_children(n, wrap_child, NULL);
  }
}
static void check_expression_wrapped(Ast *node) {
  WRAP_START = node;
  wrap_visit(node);
}
static void check_program_wrapped(Program *p) {
  FIN_CB = check_expression_wrapped;
  for (int i = 0; i < p->statements.n; i++) fin_visit(p->statements.v[i]);
}

/* ---- HighZombieFinder.js: a dot or index expression must have been lowered before this point ---- */
static void zombie_visit(Ast *n);
static void zombie_child(void *cx, Ast *c) { (void)cx; zombie_visit(c); }
static void zombie_visit(Ast *n) {
  if (n->tag == A_DOT || n->tag == A_INDEX)
    chk_error(cat(4, origin_string(n), ": High zombie: ", js_tostr(n), ""));
  visit_children(n, zombie_child, NULL);
}

/* The post-check passes, in Prepare.js order: checkLiteralTypes, resolveProperties, findHighZombies,
 * checkLiteralTypes, checkProgramWrapped, checkReturns, checkUnreachableCode, checkLoops, checkRecursion,
 * checkProgramWrapped, findHighZombies. */
static void post_check_passes(Program *p) {
  for (int i = 0; i < p->statements.n; i++) littype_visit(p->statements.v[i]);
  resolve_properties(p);
  for (int i = 0; i < p->statements.n; i++) zombie_visit(p->statements.v[i]);
  for (int i = 0; i < p->statements.n; i++) littype_visit(p->statements.v[i]);
  check_program_wrapped(p);
  RR_PROG = p;
  for (int i = 0; i < p->statements.n; i++) rr_visit(p->statements.v[i]);
  for (int i = 0; i < p->statements.n; i++) unreach_visit(p->statements.v[i]);
  LoopChk lc = {0, 0};
  for (int i = 0; i < p->statements.n; i++) loopchk_visit(&lc, p->statements.v[i]);
  RR_NSTACK = 0;
  for (int i = 0; i < p->statements.n; i++) rec_visit(p->statements.v[i]);
  check_program_wrapped(p);
  for (int i = 0; i < p->statements.n; i++) zombie_visit(p->statements.v[i]);
}

static void check_program(Program *p) {
  Checker c;
  memset(&c, 0, sizeof c);
  c.p = p;
  c.vertex = arena_alloc(sizeof(char *) * 256);
  c.fragment = arena_alloc(sizeof(char *) * 256);
  check_enum_owners(p);
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (is_type_statement(st)) { c.stmt = st; chk(&c, st); }
  }
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (st->tag == A_PROTOCOLDECL) { c.stmt = st; chk(&c, st); }
  }
  each_func_in_map_order(p, check_func_sig_cb, &c);
  each_func_in_map_order(p, check_func_cb, &c);
}

#endif
