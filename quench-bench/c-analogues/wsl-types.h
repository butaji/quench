/* WSL type layer (a C port of Node.unify, UnificationContext, the type classes' unifyImpl and
 * typeVariableUnify, ProtocolDecl.inherits/hasHeir, InferTypesForCall and ResolveOverloadImpl), with the
 * literal types (IntLiteralType, UintLiteralType, FloatLiteralType, DoubleLiteralType, NullType). A literal
 * expression's type is a literal type node made with it (lit_expr_new); the native kind dispatch of
 * Intrinsics.js is in wsl-intrinsics.h. Results are booleans for now: error reasons are not built yet.
 * NativeTypeInstance is not ported. */
#ifndef WSL_TYPES_H
#define WSL_TYPES_H

#include <setjmp.h>
#include "wsl-synth.h"
#include "wsl-intrinsics.h"
#include "wsl-strings.h"

/* ---- classification ---- */
/* the four GenericLiteralType classes and NullType (a Type) */
static int is_lit_type(const Ast *n) { return n->tag >= A_INTLITERALTYPE && n->tag <= A_DOUBLELITERALTYPE; }
static int is_lit_type_or_null(const Ast *n) { return is_lit_type(n) || n->tag == A_NULLTYPE; }
/* Type instanceof: the nodes that unify as types (literal types included) */
static int is_jstype(const Ast *n) { return is_type_node(n) || is_lit_type_or_null(n); }
/* isLiteral: literal types, NullType and the literal expressions */
static int is_literal_node(const Ast *n) {
  return is_lit_type_or_null(n) || (n->tag >= A_INTLITERAL && n->tag <= A_DOUBLELITERAL);
}

static int ut_is_unifiable(const Ast *n) {
  return n->tag == A_TYPEVARIABLE || n->tag == A_CONSTEXPRTYPEPARAM || is_lit_type_or_null(n);
}

/* instanceof Value for the nodes that unify against a constexpr parameter */
static int ut_is_value(const Ast *n) {
  switch (n->tag) {
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL:
  case A_VARIABLEREF: case A_VARIABLEDECL: case A_FUNCPARAMETER: case A_CONSTEXPRTYPEPARAM:
  case A_IDENTITY: case A_ENUMLITERAL:
    return 1;
  default:
    return 0;
  }
}

/* value.type for the values above */
static Ast *ut_value_type(const Ast *n) {
  switch (n->tag) {
  case A_VARIABLEDECL: return n->u.varDecl.type;
  case A_FUNCPARAMETER: case A_CONSTEXPRTYPEPARAM: return n->u.named.type;
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL: case A_NULLLITERAL:
    return n->type;
  case A_ENUMLITERAL: return n->u.enumLit.member->u.nameOnly.target;   /* EnumLiteral.type */
  default: fatal_unsupported("value type of a reference", n);
  }
  return NULL;
}

/* ProtocolRef.protocolDecl, or the ProtocolDecl itself */
static Ast *ut_pdecl(Ast *n) {
  if (n->tag == A_PROTOCOLREF) return n->u.nameOnly.target;
  return n;
}

/* ---- UnificationContext ---- */
typedef struct UCtx {
  Ast **tps; int ntps;                        /* typeParameters (a Set in JS) */
  Ast **keys, **vals; int nmap, capmap;       /* _nextMap, in insertion order */
  Ast **extra; int nextra, capextra;          /* _extraNodes */
} UCtx;

static UCtx *uc_new(Ast **tps, int ntps) {
  UCtx *c = arena_alloc(sizeof *c);
  memset(c, 0, sizeof *c);
  c->tps = tps;
  c->ntps = ntps;
  return c;
}

static int uc_idx(const UCtx *c, const Ast *k) {
  for (int i = 0; i < c->nmap; i++) if (c->keys[i] == k) return i;
  return -1;
}

static void uc_set(UCtx *c, Ast *k, Ast *v) {
  int i = uc_idx(c, k);
  if (i >= 0) { c->vals[i] = v; return; }
  if (c->nmap == c->capmap) {
    int cap = c->capmap ? c->capmap * 2 : 8;
    Ast **nk = arena_alloc(sizeof(Ast *) * cap), **nv = arena_alloc(sizeof(Ast *) * cap);
    if (c->nmap) { memcpy(nk, c->keys, sizeof(Ast *) * c->nmap); memcpy(nv, c->vals, sizeof(Ast *) * c->nmap); }
    c->keys = nk; c->vals = nv; c->capmap = cap;
  }
  c->keys[c->nmap] = k;
  c->vals[c->nmap] = v;
  c->nmap++;
}

static int uc_in_tps(const UCtx *c, const Ast *n) {
  for (int i = 0; i < c->ntps; i++) if (c->tps[i] == n) return 1;
  return 0;
}

static Ast *uc_find(UCtx *c, Ast *n) {
  int i = uc_idx(c, n);
  if (i < 0) return n;
  Ast *cur = c->vals[i];
  for (;;) {
    int j = uc_idx(c, cur);
    if (j < 0) break;
    cur = c->vals[j];
  }
  c->vals[i] = cur;   /* path compression, as JS does */
  return cur;
}

static void uc_union(UCtx *c, Ast *a, Ast *b) {
  a = uc_find(c, a);
  b = uc_find(c, b);
  if (a == b) return;
  if (!ut_is_unifiable(a)) {
    Ast *t = a; a = b; b = t;
    if (!ut_is_unifiable(a)) fatal_unsupported("union of non-unifiable nodes", a);
  }
  if (ut_is_unifiable(a) && ut_is_unifiable(b) && uc_in_tps(c, b)) {
    Ast *t = a; a = b; b = t;
  }
  uc_set(c, a, b);
}

/* nodes: keys and values in insertion order, then extras; a Set in JS */
static int uc_nodes(const UCtx *c, Ast **out) {
  int n = 0;
  for (int i = 0; i < c->nmap; i++) {
    for (int k = 0; k < 2; k++) {
      Ast *x = k ? c->vals[i] : c->keys[i];
      int seen = 0;
      for (int j = 0; j < n; j++) if (out[j] == x) { seen = 1; break; }
      if (!seen) out[n++] = x;
    }
  }
  for (int i = 0; i < c->nextra; i++) {
    int seen = 0;
    for (int j = 0; j < n; j++) if (out[j] == c->extra[i]) { seen = 1; break; }
    if (!seen) out[n++] = c->extra[i];
  }
  return n;
}

/* typeArguments(): unifiable nodes that are not type parameters */
static int uc_type_arguments(const UCtx *c, Ast **out) {
  int total = 2 * c->nmap + c->nextra;
  Ast **all = arena_alloc(sizeof(Ast *) * (total ? total : 1));
  int n = uc_nodes(c, all), m = 0;
  for (int i = 0; i < n; i++)
    if (ut_is_unifiable(all[i]) && !uc_in_tps(c, all[i])) out[m++] = all[i];
  return m;
}

/* ---- inheritance (ProtocolDecl, ProtocolRef, Type, TypeVariable) ---- */
static Ast *subst_rw(Ast *t, Ast **olds, Ast **news, int n) {
  Rw r = {olds, news, n, 0};
  return rw(&r, t);
}

/* substituteToUnification: substitute each parameter by its unification root */
static Ast *subst_unif(Ast *t, Ast **params, int np, UCtx *c) {
  Ast **news = arena_alloc(sizeof(Ast *) * (np ? np : 1));
  for (int i = 0; i < np; i++) news[i] = uc_find(c, params[i]);
  return subst_rw(t, params, news, np);
}

static int ut_unify(UCtx *c, Ast *self, Ast *other);
static int ut_inherits(Ast *t, Ast *proto);
static int proto_inherits(Ast *self, Ast *other);
static int proto_has_heir(Ast *proto, Ast *type);

/* ---- overload resolution (InferTypesForCall, ResolveOverloadImpl) ---- */
/* Reasons of the failing checks (the JS {result: false, reason}). A failing check stores its reason and
 * returns 0; a caller that wraps an inner reason reads it right after the inner call. */
static const char *UT_REASON = NULL;
static int ut_fail(const char *reason) { UT_REASON = reason; return 0; }

static const char *str_join(const char **v, int n, const char *sep) {
  const char *s = "";
  for (int i = 0; i < n; i++) s = CAT(s, i ? sep : "", v[i]);
  return s;
}

/* Overload: a match (func, ctx, targs); a failure of one candidate carries its reason; a failure of
 * resolve_overload carries the OverloadResolutionFailure strings of every candidate (fails). */
typedef struct {
  Ast *func;      /* NULL on failure */
  UCtx *ctx;
  Ast **targs;
  int ntargs;
  const char *reason;
  const char **fails;
  int nfails;
} Overload;

static int uc_verify_pub(UCtx *c);

static Ast *func_ret_for_overload(const Ast *f) { return f->u.func.isCast ? f->u.func.returnType : NULL; }

static Overload overload_fail(const char *reason) {
  Overload o = {0};
  o.reason = reason;
  return o;
}

/* inferTypesForCall */
static Overload infer_types_for_call(Ast *func, Ast **targs, int ntargs, Ast **args, int nargs, Ast *ret) {
  AstList *tps = &func->u.func.typeParameters, *params = &func->u.func.parameters;
  if (ntargs && ntargs != tps->n)
    return overload_fail(CAT("Wrong number of type arguments (passed ", js_num(ntargs), ", require ", js_num(tps->n), ")"));
  if (nargs != params->n)
    return overload_fail(CAT("Wrong number of arguments (passed ", js_num(nargs), ", require ", js_num(params->n), ")"));
  /* typeParametersForCallResolution: a protocol signature also unifies its protocol's type variable */
  Ast **ctps = tps->v;
  int nctps = tps->n;
  if (func->tag == A_PROTOCOLFUNCDECL && func->u.func.protocolDecl) {
    ctps = arena_alloc(sizeof(Ast *) * (tps->n + 1));
    for (int i = 0; i < tps->n; i++) ctps[i] = tps->v[i];
    ctps[tps->n] = func->u.func.protocolDecl->u.protocolDecl.typeVariable;
    nctps = tps->n + 1;
  }
  UCtx *c = uc_new(ctps, nctps);
  for (int i = 0; i < ntargs; i++)
    if (!ut_unify(c, targs[i], tps->v[i]))
      return overload_fail(CAT("Type argument #", js_num(i + 1), " for parameter ", js_tostr(tps->v[i]),
                               " does not match (passed ", js_tostr(targs[i]), ", require ", js_tostr(tps->v[i]), ")"));
  for (int i = 0; i < nargs; i++) {
    if (!args[i]) fatal_unsupported("null argument type", func);
    Ast *p = params->v[i];
    if (!ut_unify(c, args[i], p->u.named.type)) {
      const char *pname = p->u.named.name ? CAT("for parameter ", p->u.named.name, " ") : "";
      return overload_fail(CAT("Argument #", js_num(i + 1), " ", pname, "does not match (passed ", js_tostr(args[i]),
                               ", require ", js_tostr(p->u.named.type), ")"));
    }
  }
  if (ret && !ut_unify(c, ret, func->u.func.returnType))
    return overload_fail(CAT("Return type ", js_tostr(func->u.func.returnType), " does not match ", js_tostr(ret)));
  if (!uc_verify_pub(c)) return overload_fail(UT_REASON);
  Ast **out = targs;
  int shouldBuild = !ntargs;
  if (shouldBuild) out = arena_alloc(sizeof(Ast *) * (tps->n ? tps->n : 1));
  for (int i = 0; i < tps->n; i++) {
    Ast *ta = uc_find(c, tps->v[i]);
    if (ta == tps->v[i]) return overload_fail(CAT("Type parameter ", js_tostr(tps->v[i]), " did not get assigned a type"));
    if (shouldBuild) out[i] = ta;
  }
  Overload r = {func, c, out, shouldBuild ? tps->n : ntargs, NULL, NULL, 0};
  return r;
}

/* OverloadResolutionFailure: the declaration and the reason */
static const char *overload_failure_string(const Ast *f, const char *reason) {
  return CAT(js_decl(f), " did not match because: ", reason);
}

static UCtx *ut_equals(Ast *a, Ast *b);

/* UnificationContext.conversionCost: a literal type argument costs 0 when its root equals its preferred type
 * (GenericLiteralType.conversionCost), 1 otherwise; the other type arguments cost 0. */
static int uc_conversion_cost(UCtx *c) {
  int total = 2 * c->nmap + c->nextra;
  Ast **nodes = arena_alloc(sizeof(Ast *) * (total ? total : 1));
  int n = uc_nodes(c, nodes), cost = 0;
  for (int i = 0; i < n; i++) {
    Ast *x = nodes[i];
    if (!is_lit_type(x) || uc_in_tps(c, x)) continue;
    if (!ut_equals(uc_find(c, x), literal_preferred(x->u.litType.owner))) cost++;
  }
  return cost;
}

/* resolveOverloadImpl */
static Overload resolve_overload(AstList *fs, Ast **targs, int ntargs, Ast **args, int nargs, Ast *ret, int allowEntry) {
  Overload none = {0};
  if (!fs) fatal_unsupported("null functions", NULL);
  Overload *succ = arena_alloc(sizeof(Overload) * (fs->n ? fs->n : 1));
  const char **fails = arena_alloc(sizeof(const char *) * (fs->n ? fs->n : 1));
  int ns = 0, nf = 0;
  for (int i = 0; i < fs->n; i++) {
    Ast *f = fs->v[i];
    if (!allowEntry && f->u.func.shaderType) {
      fails[nf++] = overload_failure_string(f, CAT("Function is a ", f->u.func.shaderType,
                                                   " shader, so it cannot be called from within an existing shader."));
      continue;
    }
    Overload o = infer_types_for_call(f, targs, ntargs, args, nargs, ret);
    if (o.func) succ[ns++] = o;
    else fails[nf++] = overload_failure_string(f, o.reason);
  }
  none.fails = fails;
  none.nfails = nf;
  if (!ns) return none;
  /* only the successes of the least conversion cost survive */
  int minCost = -1;
  int *cost = arena_alloc(sizeof(int) * ns);
  for (int i = 0; i < ns; i++) {
    cost[i] = uc_conversion_cost(succ[i].ctx);
    if (minCost < 0 || cost[i] < minCost) minCost = cost[i];
  }
  int kept = 0;
  for (int i = 0; i < ns; i++) if (cost[i] == minCost) succ[kept++] = succ[i];
  ns = kept;
  int anyRestricted = 0;
  for (int i = 0; i < ns; i++) if (succ[i].func->u.func.isRestricted) anyRestricted = 1;
  if (anyRestricted) {
    int m = 0;
    for (int i = 0; i < ns; i++) if (succ[i].func->u.func.isRestricted) succ[m++] = succ[i];
    ns = m;
  }
  Overload *pruned = arena_alloc(sizeof(Overload) * (ns ? ns : 1));
  int np = 0;
  for (int i = 0; i < ns; i++) {
    int ok = 1;
    Ast *argFunc = succ[i].func;
    for (int j = 0; j < ns; j++) {
      if (i == j) continue;
      Ast *parFunc = succ[j].func;
      Ast **pts = arena_alloc(sizeof(Ast *) * (argFunc->u.func.parameters.n ? argFunc->u.func.parameters.n : 1));
      for (int k = 0; k < argFunc->u.func.parameters.n; k++) pts[k] = argFunc->u.func.parameters.v[k]->u.named.type;
      Overload o = infer_types_for_call(parFunc,
                                        ntargs ? argFunc->u.func.typeParameters.v : NULL,
                                        ntargs ? argFunc->u.func.typeParameters.n : 0,
                                        pts, argFunc->u.func.parameters.n,
                                        func_ret_for_overload(argFunc));
      if (!o.func) { ok = 0; break; }
    }
    if (ok) pruned[np++] = succ[i];
  }
  if (np == 1) return pruned[0];
  /* ambiguity: every candidate of the list fails with the same message */
  const char *message;
  Overload *list = pruned;
  int nl = np;
  if (np == 0) {
    message = "Ambiguous overload - no function can be applied to all others";
    list = succ;
    nl = ns;
  } else {
    message = "Ambiguous overload - functions mutually applicable";
  }
  const char **afails = arena_alloc(sizeof(const char *) * (nl ? nl : 1));
  for (int i = 0; i < nl; i++) afails[i] = overload_failure_string(list[i].func, message);
  none.fails = afails;
  none.nfails = nl;
  return none;
}

/* ---- unify, typeVariableUnify, unifyImpl ---- */
static Ast *ut_unify_node(Ast *n) {
  switch (n->tag) {
  case A_TYPEREF:
    if (!n->u.typeRef.typeArguments.n) return ut_unify_node(n->u.typeRef.type);
    return n;
  case A_IDENTITY: return ut_unify_node(n->u.identity.target);
  case A_VARIABLEREF: return ut_unify_node(n->u.varRef.variable);
  default: return n;
  }
}

static int ut_tv_impl(UCtx *c, Ast *self, Ast *other) {
  Ast *realThis = uc_find(c, self);
  if (realThis != self) return ut_unify(c, realThis, other);
  uc_union(c, self, other);
  return 1;
}

/* other.typeVariableUnify(c, self) */
static int ut_typevar_unify(UCtx *c, Ast *self, Ast *other) {
  switch (self->tag) {
  case A_TYPEVARIABLE:
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
  case A_NULLTYPE:
    if (!is_jstype(other)) return 0;
    return ut_tv_impl(c, self, other);
  case A_CONSTEXPRTYPEPARAM:
    if (!ut_is_value(other)) return 0;
    if (!ut_unify(c, self->u.named.type, ut_value_type(other))) return 0;
    return ut_tv_impl(c, self, other);
  default:
    return 0;
  }
}

static int ut_unify_impl(UCtx *c, Ast *self, Ast *other) {
  switch (self->tag) {
  case A_TYPEREF:
    if (other->tag != A_TYPEREF) return 0;
    if (!ut_unify(c, self->u.typeRef.type, other->u.typeRef.type)) return 0;
    if (self->u.typeRef.typeArguments.n != other->u.typeRef.typeArguments.n) return 0;
    for (int i = 0; i < self->u.typeRef.typeArguments.n; i++)
      if (!ut_unify(c, self->u.typeRef.typeArguments.v[i], other->u.typeRef.typeArguments.v[i])) return 0;
    return 1;
  case A_TYPEVARIABLE:
  case A_CONSTEXPRTYPEPARAM:
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
  case A_NULLTYPE:
    return ut_typevar_unify(c, self, other);
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    if (other->tag != self->tag) return 0;
    if (strcmp(self->u.ref.addressSpace, other->u.ref.addressSpace) != 0) return 0;
    return ut_unify(c, self->u.ref.elementType, other->u.ref.elementType);
  case A_ARRAYTYPE:
    if (other->tag != A_ARRAYTYPE) return 0;
    if (!ut_unify(c, self->u.arrayType.numElements, other->u.arrayType.numElements)) return 0;
    return ut_unify(c, self->u.arrayType.elementType, other->u.arrayType.elementType);
  case A_INTLITERAL:
  case A_UINTLITERAL:
    return other->tag == self->tag && self->u.intLit.value == other->u.intLit.value;
  case A_FLOATLITERAL:
  case A_DOUBLELITERAL:
    return other->tag == self->tag && self->u.fltLit.value == other->u.fltLit.value;
  case A_ENUMLITERAL:   /* EnumLiteral.unifyImpl: the same member (Node.unify already tried the typevar) */
    return other->tag == A_ENUMLITERAL && self->u.enumLit.member == other->u.enumLit.member;
  default:
    if (ut_typevar_unify(c, other, self)) return 1;
    return self == other;
  }
}

/* A unification that does not terminate (there is no occurs check, so a variable can be bound to a
 * type that contains it) overflows the stack in JS. The harness reports those as E; here a depth
 * limit stands in for the stack overflow. */
static int UT_DEPTH = 0;
static jmp_buf UT_JMP;
static int UT_ARMED = 0;
static int ut_unify_inner(UCtx *c, Ast *self, Ast *other);
static int ut_unify(UCtx *c, Ast *self, Ast *other) {
  if (UT_DEPTH >= 2000) {
    /* the RangeError unwinds to the caller at once, as in JS */
    if (UT_ARMED) longjmp(UT_JMP, 1);
    fatal_unsupported("unification depth", self);
  }
  UT_DEPTH++;
  int r = ut_unify_inner(c, self, other);
  UT_DEPTH--;
  return r;
}

static int ut_unify_inner(UCtx *c, Ast *self, Ast *other) {
  if (!other) fatal_unsupported("null other in unify", self);
  Ast *a = ut_unify_node(self);
  Ast *b = ut_unify_node(other);
  if (a == b) return 1;
  if (ut_typevar_unify(c, b, a)) return 1;
  return ut_unify_impl(c, a, b);
}

/* ---- verify ---- */
/* GenericLiteralType.verifyAsArgument and NullType.verifyAsArgument, on the root of the literal */
static int lit_verify_as_argument(UCtx *c, Ast *n) {
  Ast *realThis = uc_find(c, n);
  if (n->tag == A_NULLTYPE) {
    if (realThis->tag == A_PTRTYPE || realThis->tag == A_ARRAYREFTYPE) return 1;
    return ut_fail(CAT("Null cannot be used with non-pointer type ", js_tostr(realThis)));
  }
  double v = lit_value(n);
  const char *val = js_num(v);
  switch (n->tag) {
  case A_INTLITERALTYPE:
    if (!ty_is_number(realThis)) return ut_fail(CAT("Cannot use int literal with non-number type ", js_tostr(realThis)));
    if (!nt_can_represent(nt_kind(realThis), v))
      return ut_fail(CAT("Int literal ", val, " too large to be represented by type ", js_tostr(realThis)));
    return 1;
  case A_UINTLITERALTYPE:
    if (!ty_is_int(realThis)) return ut_fail(CAT("Cannot use uint literal with non-integer type ", js_tostr(realThis)));
    if (ty_is_signed(realThis)) return ut_fail(CAT("Cannot use uint literal with signed type ", js_tostr(realThis)));
    if (!nt_can_represent(nt_kind(realThis), v))
      return ut_fail(CAT("Uint literal ", val, " too large to be represented by type ", js_tostr(realThis)));
    return 1;
  case A_FLOATLITERALTYPE:
    if (!ty_is_floating(realThis)) return ut_fail(CAT("Cannot use float literal with non-floating type ", js_tostr(realThis)));
    if (!nt_can_represent(nt_kind(realThis), v))
      return ut_fail(CAT("Float literal ", val, " does not fit in type ", js_tostr(realThis)));
    return 1;
  case A_DOUBLELITERALTYPE:
    if (!ty_is_floating(realThis)) return ut_fail(CAT("Cannot use double literal with non-floating type ", js_tostr(realThis)));
    if (!nt_can_represent(nt_kind(realThis), v))
      return ut_fail(CAT("Float literal ", val, " does not fit in type ", js_tostr(realThis)));
    return 1;
  default: fatal_unsupported("verifyAsArgument of a literal type", n);
  }
  return 0;
}

static int ut_verify_as_argument(UCtx *c, Ast *n) {
  switch (n->tag) {
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE:
  case A_NULLTYPE:
    return lit_verify_as_argument(c, n);
  case A_TYPEVARIABLE: {
    Ast *realThis = uc_find(c, n);
    if (realThis->tag != A_TYPEVARIABLE)
      return ut_fail(CAT("Type variable argument ", js_tostr(n), " cannot be passed to non-type-variable parameter type ",
                         js_tostr(realThis)));
    if (!n->u.typeVariable.protocol) {
      if (realThis->u.typeVariable.protocol)
        return ut_fail(CAT("Type variable without protocol ", js_tostr(n),
                           " cannot be passed to parameter type variable with protocol ",
                           proto_name(realThis->u.typeVariable.protocol)));
      return 1;
    }
    if (proto_inherits(n->u.typeVariable.protocol, realThis->u.typeVariable.protocol)) return 1;
    const char *inner = UT_REASON;
    return ut_fail(CAT("Protocol ", proto_name(n->u.typeVariable.protocol), " does not subsume protocol ",
                       proto_name(realThis->u.typeVariable.protocol), " (passing type ", js_tostr(n), " to type ",
                       js_tostr(realThis), "): ", inner));
  }
  case A_CONSTEXPRTYPEPARAM:
    return 1;
  default:
    fatal_unsupported("verifyAsArgument", n);
  }
  return 0;
}

static int ut_verify_as_parameter(UCtx *c, Ast *n) {
  switch (n->tag) {
  case A_TYPEVARIABLE: {
    if (!n->u.typeVariable.protocol) return 1;
    Ast *realThis = uc_find(c, n);
    if (!is_jstype(realThis)) fatal_unsupported("non-type parameter binding", realThis);
    if (ut_inherits(realThis, n->u.typeVariable.protocol)) return 1;
    const char *inner = UT_REASON;
    return ut_fail(CAT("Type ", js_tostr(realThis), " does not inherit protocol ", proto_name(n->u.typeVariable.protocol),
                       " (passing type ", js_tostr(realThis), " to type ", js_tostr(n), "): ", inner));
  }
  case A_CONSTEXPRTYPEPARAM:
    return 1;
  default:
    fatal_unsupported("verifyAsParameter", n);
  }
  return 0;
}

/* UnificationContext.verify. Two phases: a literal type whose root is a type variable or a literal
 * prepares to unify that root with its preferred type (GenericLiteralType.prepareToVerify); then the
 * type parameters and the arguments are verified. Literal arguments are not counted as variables. */
typedef struct { Ast *root; Ast *pref; } LitPrep;
static int uc_verify_pub(UCtx *c) {
  int total = 2 * c->nmap + c->nextra;
  Ast **nodes = arena_alloc(sizeof(Ast *) * (total ? total : 1));
  int nn = uc_nodes(c, nodes);
  LitPrep *preps = arena_alloc(sizeof(LitPrep) * (nn ? nn : 1));
  int np = 0;
  for (int i = 0; i < nn; i++) {
    if (!is_lit_type(nodes[i])) continue;
    Ast *root = uc_find(c, nodes[i]);
    if (root->tag != A_TYPEVARIABLE && !is_literal_node(root)) continue;
    preps[np].root = root;
    preps[np].pref = literal_preferred(nodes[i]->u.litType.owner);
    np++;
  }
  for (int i = 0; i < np; i++)
    if (!ut_unify(c, preps[i].root, preps[i].pref))
      return ut_fail(CAT("Type mismatch between ", js_tostr(uc_find(c, preps[i].root)), " and ", js_tostr(preps[i].pref)));
  for (int i = 0; i < c->ntps; i++)
    if (!ut_verify_as_parameter(c, c->tps[i])) return 0;
  Ast **args = arena_alloc(sizeof(Ast *) * (total ? total : 1));
  int nargs = uc_type_arguments(c, args);
  Ast **set = arena_alloc(sizeof(Ast *) * (nargs ? nargs : 1));
  int nset = 0, nvar = 0;
  for (int i = 0; i < nargs; i++) {
    if (!ut_verify_as_argument(c, args[i])) return 0;
    if (is_literal_node(args[i])) continue;
    Ast *r = uc_find(c, args[i]);
    int seen = 0;
    for (int j = 0; j < nset; j++) if (set[j] == r) { seen = 1; break; }
    if (!seen) set[nset++] = r;
    nvar++;
  }
  if (nset == nvar) return 1;
  return ut_fail("Type variables used as arguments got unified with each other");
}

/* Node.equals: a context when the two types unify and verify, else NULL */
static UCtx *ut_equals(Ast *a, Ast *b) {
  UCtx *c = uc_new(NULL, 0);
  if (ut_unify(c, a, b) && uc_verify_pub(c)) return c;
  return NULL;
}

/* Type.inherits / TypeVariable.inherits */
static int ut_inherits(Ast *t, Ast *proto) {
  if (!proto) return 1;
  if (t->tag == A_TYPEVARIABLE) {
    if (!t->u.typeVariable.protocol) return ut_fail(CAT("Type variable ", js_tostr(t), " does not have a protocol"));
    return proto_inherits(t->u.typeVariable.protocol, proto);
  }
  return proto_has_heir(proto, t);
}

/* ProtocolDecl.inherits: self has every signature of other */
static int proto_inherits(Ast *self_proto, Ast *other) {
  Ast *self = ut_pdecl(self_proto);
  if (!other) return 1;
  other = ut_pdecl(other);
  for (int i = 0; i < other->u.protocolDecl.signatures.n; i++) {
    Ast *os = other->u.protocolDecl.signatures.v[i];
    /* the overload list is the protocol's own signatures with this name, in order */
    AstList named = {0};
    for (int k = 0; k < self->u.protocolDecl.signatures.n; k++) {
      Ast *s = self->u.protocolDecl.signatures.v[k];
      if (strcmp(s->u.func.name, os->u.func.name) == 0) list_push(&named, s);
    }
    if (!named.n)
      return ut_fail(CAT("Protocol ", self->u.protocolDecl.name, " does not have a function named ", os->u.func.name,
                         " (looking at signature ", js_tostr(os), ")"));
    Ast **pts = arena_alloc(sizeof(Ast *) * (os->u.func.parameters.n ? os->u.func.parameters.n : 1));
    for (int k = 0; k < os->u.func.parameters.n; k++) pts[k] = os->u.func.parameters.v[k]->u.named.type;
    Overload o = resolve_overload(&named, NULL, 0, pts, os->u.func.parameters.n, func_ret_for_overload(os), 0);
    if (!o.func)
      return ut_fail(CAT("Did not find matching signature for ", js_tostr(os), " in ", self->u.protocolDecl.name,
                         o.nfails ? CAT(" (tried: ", str_join(o.fails, o.nfails, "; "), ")") : ""));
    Ast *sub = subst_unif(o.func->u.func.returnType, o.func->u.func.typeParameters.v, o.func->u.func.typeParameters.n, o.ctx);
    if (!ut_equals(sub, os->u.func.returnType))
      return ut_fail(CAT("Return type mismatch between ", js_tostr(os->u.func.returnType), " and ", js_tostr(sub)));
  }
  return 1;
}

/* ProtocolDecl.hasHeir: substitute the protocol's type variable and resolve every signature */
static int proto_has_heir(Ast *proto, Ast *type) {
  Ast *p = ut_pdecl(proto);
  Ast *tv = p->u.protocolDecl.typeVariable;
  for (int i = 0; i < p->u.protocolDecl.signatures.n; i++) {
    Ast *orig = p->u.protocolDecl.signatures.v[i];
    Ast *olds[1] = {tv};
    Ast *news[1] = {type};
    Ast *sig = subst_rw(orig, olds, news, 1);
    Ast **pts = arena_alloc(sizeof(Ast *) * (sig->u.func.parameters.n ? sig->u.func.parameters.n : 1));
    for (int k = 0; k < sig->u.func.parameters.n; k++) pts[k] = sig->u.func.parameters.v[k]->u.named.type;
    Overload o = resolve_overload(sig->u.func.ov, sig->u.func.typeParameters.v, sig->u.func.typeParameters.n,
                                  pts, sig->u.func.parameters.n, func_ret_for_overload(sig), 0);
    if (!o.func)
      return ut_fail(CAT("Did not find matching signature for ", js_tostr(orig), " (at ", origin_string(orig),
                         ") with type ", js_tostr(type), o.nfails ? CAT(" (tried: ", str_join(o.fails, o.nfails, "; "), ")") : ""));
    Ast *sub = subst_unif(o.func->u.func.returnType, o.func->u.func.typeParameters.v, o.func->u.func.typeParameters.n, o.ctx);
    if (!ut_equals(sub, sig->u.func.returnType))
      return ut_fail(CAT("At signature ", js_tostr(orig), " (at ", origin_string(orig), "): return type mismatch between ",
                         js_tostr(sig->u.func.returnType), " and ", js_tostr(sub), " in found function ",
                         js_decl(o.func)));
  }
  return 1;
}

/* ---- unify harness (--unify): the first 60 types in a fixed order, one character per ordered pair:
 * bit 0 is Node.unify, bit 1 is Node.equals. Order: struct fields (statement order), function
 * parameter and return types (functions map order), protocol signature types (statement order). */
typedef struct { Ast **out; int n; int cap; } TypeList;
/* the types the unify harness covers: any type whose element types are plain (array sizes, constexpr
 * parameters and literals inside them unify as values, with their literal types) */
static int ut_plain(const Ast *t) {
  switch (t->tag) {
  case A_TYPEREF:
    for (int i = 0; i < t->u.typeRef.typeArguments.n; i++)
      if (!ut_plain(t->u.typeRef.typeArguments.v[i])) return 0;
    return 1;
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    return ut_plain(t->u.ref.elementType);
  case A_ARRAYTYPE:
    return ut_plain(t->u.arrayType.elementType);
  case A_TYPEVARIABLE: case A_STRUCTTYPE: case A_ENUMTYPE: case A_NATIVETYPE:
    return 1;
  default:
    return 0;
  }
}
static void tl_push(TypeList *t, Ast *x) {
  if (t->n < t->cap && ut_plain(x)) t->out[t->n++] = x;
}
static void harness_fn(void *cx, Ast *f) {
  TypeList *t = cx;
  for (int i = 0; i < f->u.func.parameters.n; i++) tl_push(t, f->u.func.parameters.v[i]->u.named.type);
  tl_push(t, f->u.func.returnType);
}
/* the first 60 plain types of the input, in the harness order (__harnessTypes in the node reference) */
static TypeList harness_types(Program *p) {
  enum { CAP = 60 };
  TypeList tl = {arena_alloc(sizeof(Ast *) * CAP), 0, CAP};
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (st->tag != A_STRUCTTYPE) continue;
    for (int k = 0; k < st->u.structType.fields.n; k++) tl_push(&tl, st->u.structType.fields.v[k]->u.named.type);
  }
  each_func_in_map_order(p, harness_fn, &tl);
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (st->tag != A_PROTOCOLDECL) continue;
    for (int k = 0; k < st->u.protocolDecl.signatures.n; k++) {
      Ast *s = st->u.protocolDecl.signatures.v[k];
      for (int j = 0; j < s->u.func.parameters.n; j++) tl_push(&tl, s->u.func.parameters.v[j]->u.named.type);
      tl_push(&tl, s->u.func.returnType);
    }
  }
  return tl;
}

/* one cell of a harness row: bit 0 is unify(a, b), bit 1 is equals(a, b); E on an unwound stack overflow */
static void harness_cell(Buf *b, Ast *a, Ast *x) {
  char one[2] = {'E', 0};
  UT_DEPTH = 0;
  UT_ARMED = 1;
  if (setjmp(UT_JMP) == 0) {
    UCtx *c = uc_new(NULL, 0);
    int u = ut_unify(c, a, x);
    int e = ut_equals(a, x) != NULL;
    one[0] = (char)('0' + (u ? 1 : 0) + (e ? 2 : 0));
  }
  UT_ARMED = 0;
  buf_str(b, one);
}

static void unify_harness(Program *p, Buf *b) {
  TypeList tl = harness_types(p);
  char head[32];
  snprintf(head, sizeof head, "types %d\n", tl.n);
  buf_str(b, head);
  for (int i = 0; i < tl.n; i++) {
    for (int j = 0; j < tl.n; j++) harness_cell(b, tl.out[i], tl.out[j]);
    buf_str(b, "\n");
  }
}

/* --literal: each literal type (the four GenericLiteralTypes with the values below, and NullType) against
 * every harness type. A literal's preferred type is the native type its name gives (int32, uint32,
 * float32, float64), resolved here as name resolution does for literals in the program. */
static Ast *native_named(Program *p, const char *name) {
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (st->tag == A_NATIVETYPE && strcmp(st->u.nativeType.name, name) == 0) return st;
  }
  return NULL;
}

static void literal_harness(Program *p, Buf *b) {
  TypeList tl = harness_types(p);
  static const long long INT_VALUES[] = {0, 1, 255, 256, -1, 65535, 2147483647LL, -2147483648LL,
                                         2147483648LL, 4294967295LL, 4294967296LL, 16777217LL, 9007199254740992LL};
  static const double FLT_VALUES[] = {0, 0.5, -2.5, 0.1, 16777217, 3.4e38, 1e40, 4294967296.0, -0.25};
  static const char *PREF[4] = {"int32", "uint32", "float32", "float64"};
  static const AstTag TAGS[4] = {A_INTLITERAL, A_UINTLITERAL, A_FLOATLITERAL, A_DOUBLELITERAL};
  char head[32];
  snprintf(head, sizeof head, "literal %d\n", tl.n);
  buf_str(b, head);
  for (int ki = 0; ki < 4; ki++) {
    int nv = ki < 2 ? (int)(sizeof INT_VALUES / sizeof *INT_VALUES) : (int)(sizeof FLT_VALUES / sizeof *FLT_VALUES);
    for (int vi = 0; vi < nv; vi++) {
      Ast *e = lit_expr_new(TAGS[ki], 0);
      if (ki < 2) e->u.intLit.value = INT_VALUES[vi];
      else e->u.fltLit.value = FLT_VALUES[vi];
      literal_preferred(e)->u.typeRef.type = native_named(p, PREF[ki]);
      buf_fmt(b, "%d %d ", ki, vi);
      for (int j = 0; j < tl.n; j++) harness_cell(b, e->type, tl.out[j]);
      buf_str(b, "\n");
    }
  }
  Ast *nl = lit_expr_new(A_NULLLITERAL, 0);
  buf_str(b, "4 0 ");
  for (int j = 0; j < tl.n; j++) harness_cell(b, nl->type, tl.out[j]);
  buf_str(b, "\n");
}


/* --intrinsics: the same properties of each native type, printed as IEEE bits (the properties Intrinsics.js
 * defines for the kind; the others are not called). */
static void hex_bits(Buf *b, double v) {
  uint64_t x;
  memcpy(&x, &v, 8);
  buf_fmt(b, "%016llx", (unsigned long long)x);
}

static void intrinsics_harness(Program *p, Buf *b) {
  static const double VALUES[] = {0, 1, -1, 255, 256, 65535, 2147483647, -2147483648.0, 2147483648.0,
                                  4294967295.0, 4294967296.0, 16777217, 0.5, -2.5, 3.4e38, 1e40, -0.25};
  static const char *NAMES[7] = {"void", "int32", "uint32", "uint8", "float32", "float64", "bool"};
  static const char *FMT[4] = {"IntLiteral", "UintLiteral", "FloatLiteral", "DoubleLiteral"};
  int nv = (int)(sizeof VALUES / sizeof *VALUES);
  for (int ni = 0; ni < 7; ni++) {
    NtKind k = nt_kind(native_named(p, NAMES[ni]));
    const char *name = NAMES[ni];
    int isInt = nt_is_int(k);
    int canRep = isInt || k == NT_FLOAT32 || k == NT_FLOAT64;
    int fmtOk[4];
    for (int f = 0; f < 4; f++) fmtOk[f] = isInt ? f < 2 : nt_is_floating(k);
    buf_fmt(b, "%s size %d\n", name, nt_size(k));
    if (isInt) { buf_fmt(b, "%s default ", name); hex_bits(b, nt_default_value(k)); buf_str(b, "\n"); }
    for (int vi = 0; vi < nv; vi++) {
      double v = VALUES[vi];
      if (canRep) { buf_fmt(b, "%s canRepresent ", name); hex_bits(b, v); buf_fmt(b, " %d\n", nt_can_represent(k, v)); }
      if (isInt) {
        buf_fmt(b, "%s createLiteral ", name); hex_bits(b, v); buf_str(b, " ");
        hex_bits(b, nt_create_literal(k, v)); buf_str(b, "\n");
        buf_fmt(b, "%s successorValue ", name); hex_bits(b, v); buf_str(b, " ");
        hex_bits(b, nt_successor(k, v)); buf_str(b, "\n");
      }
      for (int f = 0; f < 4; f++) {
        if (!fmtOk[f]) continue;
        buf_fmt(b, "%s formatValue %s ", name, FMT[f]); hex_bits(b, v); buf_str(b, " ");
        hex_bits(b, nt_format_value(k, (LitKind)f, v)); buf_str(b, "\n");
      }
    }
    if (isInt)
      for (int a = 0; a < nv; a++)
        for (int c = 0; c < nv; c++) {
          buf_fmt(b, "%s valuesEqual ", name); hex_bits(b, VALUES[a]); buf_str(b, " "); hex_bits(b, VALUES[c]);
          buf_fmt(b, " %d\n", nt_values_equal(k, VALUES[a], VALUES[c]));
        }
  }
}

/* --reasons: see __reasonsHarness in tools/wsl-ast-dump.cjs. */
static void reasons_harness(Program *p, Buf *b) {
  TypeList tl = harness_types(p);
  buf_fmt(b, "types %d\n", tl.n);
  for (int i = 0; i < tl.n; i++) buf_fmt(b, "T %d %s\n", i, js_tostr(tl.out[i]));
  int np = tl.n < 15 ? tl.n : 15;
  for (int i = 0; i < np; i++)
    for (int j = 0; j < np; j++) {
      const char *r;
      UT_DEPTH = 0;
      UT_ARMED = 1;
      if (setjmp(UT_JMP)) {
        r = "E";
      } else {
        UCtx *c = uc_new(NULL, 0);
        if (!ut_unify(c, tl.out[i], tl.out[j])) r = "nounify";
        else r = uc_verify_pub(c) ? "ok" : UT_REASON;
      }
      UT_ARMED = 0;
      buf_fmt(b, "P %d %d %s\n", i, j, r);
    }
}

typedef struct { Program *p; Buf *b; } ReasonsCx;
static void reasons_fn(void *cx, Ast *f) {
  ReasonsCx *r = cx;
  AstList ovs = {0};
  for (int i = 0; i < r->p->statements.n; i++) {
    Ast *st = r->p->statements.v[i];
    if ((st->tag == A_FUNCDEF || st->tag == A_NATIVEFUNC) && strcmp(st->u.func.name, f->u.func.name) == 0)
      list_push(&ovs, st);
  }
  int np = f->u.func.parameters.n;
  Ast **args = arena_alloc(sizeof(Ast *) * (np ? np : 1));
  int ntp = f->u.func.typeParameters.n;
  Ast **fresh = arena_alloc(sizeof(Ast *) * (ntp ? ntp : 1));
  for (int i = 0; i < ntp; i++) {
    Ast *tp = f->u.func.typeParameters.v[i];
    fresh[i] = mk_from(A_TYPEVARIABLE, tp);
    fresh[i]->u.typeVariable.name = tp->u.typeVariable.name;
    fresh[i]->u.typeVariable.protocol = tp->u.typeVariable.protocol;
  }
  for (int i = 0; i < np; i++)
    args[i] = rw_aw(subst_rw(f->u.func.parameters.v[i]->u.named.type, f->u.func.typeParameters.v, fresh, ntp));
  UT_DEPTH = 0;
  UT_ARMED = 1;
  if (setjmp(UT_JMP)) { UT_ARMED = 0; buf_str(r->b, "E\n"); return; }
  Overload o = resolve_overload(&ovs, NULL, 0, args, np, NULL, 0);
  UT_ARMED = 0;
  if (o.func) return;
  for (int k = 0; k < o.nfails; k++) buf_fmt(r->b, "F %s\n", o.fails[k]);
}

static void reasons_harness_all(Program *p, Buf *b) {
  reasons_harness(p, b);
  ReasonsCx cx = {p, b};
  each_func_in_map_order(p, reasons_fn, &cx);
}

/* ---- resolve harness (--resolve): for each function in map order, resolve its own parameter types
 * (AutoWrapped, as CallExpression.resolve does) against its overload list. One line per function:
 * the chosen overload's index in the list (-1 if none) and 1 if the substituted return type equals
 * the declared one (0 otherwise, or "-" with no choice). */
typedef struct { Program *p; Buf *b; int n; } ResCx;
static void resolve_fn(void *cx, Ast *f) {
  ResCx *r = cx;
  AstList ovs = {0};
  for (int i = 0; i < r->p->statements.n; i++) {
    Ast *st = r->p->statements.v[i];
    if ((st->tag == A_FUNCDEF || st->tag == A_NATIVEFUNC) && strcmp(st->u.func.name, f->u.func.name) == 0)
      list_push(&ovs, st);
  }
  int np = f->u.func.parameters.n;
  Ast **args = arena_alloc(sizeof(Ast *) * (np ? np : 1));
  /* the arguments get fresh type variables for the function's own type parameters (a call never passes
   * them), so that no variable is unified with a type that contains it */
  int ntp = f->u.func.typeParameters.n;
  Ast **fresh = arena_alloc(sizeof(Ast *) * (ntp ? ntp : 1));
  for (int i = 0; i < ntp; i++) {
    Ast *tp = f->u.func.typeParameters.v[i];
    fresh[i] = mk_from(A_TYPEVARIABLE, tp);
    fresh[i]->u.typeVariable.name = tp->u.typeVariable.name;
    fresh[i]->u.typeVariable.protocol = tp->u.typeVariable.protocol;
  }
  for (int i = 0; i < np; i++)
    args[i] = rw_aw(subst_rw(f->u.func.parameters.v[i]->u.named.type, f->u.func.typeParameters.v, fresh, ntp));
  r->n++;
  UT_DEPTH = 0;
  UT_ARMED = 1;
  if (setjmp(UT_JMP)) { UT_ARMED = 0; buf_str(r->b, "E\n"); return; }
  Overload o = resolve_overload(&ovs, NULL, 0, args, np, NULL, 0);
  if (!o.func) { UT_ARMED = 0; buf_str(r->b, "-1 -\n"); return; }
  int index = -1;
  for (int i = 0; i < ovs.n; i++) if (ovs.v[i] == o.func) index = i;
  Ast *sub = subst_unif(o.func->u.func.returnType, o.func->u.func.typeParameters.v, o.func->u.func.typeParameters.n, o.ctx);
  Ast *res = rw_aw(sub);
  char line[64];
  int eq = ut_equals(res, f->u.func.returnType) != NULL;
  UT_ARMED = 0;
  snprintf(line, sizeof line, "%d %d\n", index, eq);
  buf_str(r->b, line);
}
static void resolve_harness(Program *p, Buf *b) {
  ResCx cx = {p, b, 0};
  char head[32];
  int count = 0;
  for (int i = 0; i < p->statements.n; i++) {
    Ast *st = p->statements.v[i];
    if (st->tag == A_FUNCDEF || st->tag == A_NATIVEFUNC) count++;
  }
  snprintf(head, sizeof head, "funcs %d\n", count);
  buf_str(b, head);
  each_func_in_map_order(p, resolve_fn, &cx);
}

#endif
