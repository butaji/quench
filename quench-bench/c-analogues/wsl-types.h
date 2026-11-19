/* WSL type layer, run 1a (a C port of Node.unify, UnificationContext, the type classes' unifyImpl and
 * typeVariableUnify, ProtocolDecl.inherits/hasHeir, InferTypesForCall and ResolveOverloadImpl).
 * Results are booleans for now: error reasons are not built yet. The literal types (IntLiteralType
 * and friends), NullType and NativeTypeInstance are created only by the checker and are not reached
 * here; value_type reports them with fatal_unsupported. */
#ifndef WSL_TYPES_H
#define WSL_TYPES_H

#include "wsl-synth.h"

/* ---- classification ---- */
static int ut_is_unifiable(const Ast *n) {
  return n->tag == A_TYPEVARIABLE || n->tag == A_CONSTEXPRTYPEPARAM;
}

/* instanceof Value for the nodes that unify against a constexpr parameter */
static int ut_is_value(const Ast *n) {
  switch (n->tag) {
  case A_INTLITERAL: case A_UINTLITERAL: case A_FLOATLITERAL: case A_DOUBLELITERAL:
  case A_VARIABLEREF: case A_VARIABLEDECL: case A_FUNCPARAMETER: case A_CONSTEXPRTYPEPARAM:
  case A_IDENTITY:
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
  default: fatal_unsupported("value type of a literal or reference (literal types are not ported)", n);
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
typedef struct {
  Ast *func;      /* NULL on failure */
  UCtx *ctx;
  Ast **targs;
  int ntargs;
} Overload;

static int uc_verify_pub(UCtx *c);

static Ast *func_ret_for_overload(const Ast *f) { return f->u.func.isCast ? f->u.func.returnType : NULL; }

static Overload infer_types_for_call(Ast *func, Ast **targs, int ntargs, Ast **args, int nargs, Ast *ret) {
  Overload none = {NULL, NULL, NULL, 0};
  AstList *tps = &func->u.func.typeParameters, *params = &func->u.func.parameters;
  if (ntargs && ntargs != tps->n) return none;
  if (nargs != params->n) return none;
  UCtx *c = uc_new(tps->v, tps->n);
  for (int i = 0; i < ntargs; i++)
    if (!ut_unify(c, targs[i], tps->v[i])) return none;
  for (int i = 0; i < nargs; i++) {
    if (!args[i]) fatal_unsupported("null argument type", func);
    if (!ut_unify(c, args[i], params->v[i]->u.named.type)) return none;
  }
  if (ret && !ut_unify(c, ret, func->u.func.returnType)) return none;
  if (!uc_verify_pub(c)) return none;
  Ast **out = targs;
  int shouldBuild = !ntargs;
  if (shouldBuild) out = arena_alloc(sizeof(Ast *) * (tps->n ? tps->n : 1));
  for (int i = 0; i < tps->n; i++) {
    Ast *ta = uc_find(c, tps->v[i]);
    if (ta == tps->v[i]) return none;
    if (shouldBuild) out[i] = ta;
  }
  Overload r = {func, c, out, shouldBuild ? tps->n : ntargs};
  return r;
}

static Overload resolve_overload(AstList *fs, Ast **targs, int ntargs, Ast **args, int nargs, Ast *ret, int allowEntry) {
  Overload none = {NULL, NULL, NULL, 0};
  if (!fs) fatal_unsupported("null functions", NULL);
  Overload *succ = arena_alloc(sizeof(Overload) * (fs->n ? fs->n : 1));
  int ns = 0;
  for (int i = 0; i < fs->n; i++) {
    Ast *f = fs->v[i];
    if (!allowEntry && f->u.func.shaderType) continue;
    Overload o = infer_types_for_call(f, targs, ntargs, args, nargs, ret);
    if (o.func) succ[ns++] = o;
  }
  if (!ns) return none;
  /* conversionCost is 0 for every node reached so far (literal types are not ported), so all survive */
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
    if (!is_type_node(other)) return 0;
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
  default:
    if (ut_typevar_unify(c, other, self)) return 1;
    return self == other;
  }
}

static int ut_unify(UCtx *c, Ast *self, Ast *other) {
  if (!other) fatal_unsupported("null other in unify", self);
  Ast *a = ut_unify_node(self);
  Ast *b = ut_unify_node(other);
  if (a == b) return 1;
  if (ut_typevar_unify(c, b, a)) return 1;
  return ut_unify_impl(c, a, b);
}

/* ---- verify ---- */
static int ut_verify_as_argument(UCtx *c, Ast *n) {
  switch (n->tag) {
  case A_TYPEVARIABLE: {
    Ast *realThis = uc_find(c, n);
    if (realThis->tag != A_TYPEVARIABLE) return 0;
    if (!n->u.typeVariable.protocol) return realThis->u.typeVariable.protocol ? 0 : 1;
    return proto_inherits(n->u.typeVariable.protocol, realThis->u.typeVariable.protocol);
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
    if (!is_type_node(realThis)) fatal_unsupported("non-type parameter binding", realThis);
    return ut_inherits(realThis, n->u.typeVariable.protocol);
  }
  case A_CONSTEXPRTYPEPARAM:
    return 1;
  default:
    fatal_unsupported("verifyAsParameter", n);
  }
  return 0;
}

/* UnificationContext.verify (the prepareToVerify pass only matters for literal types) */
static int uc_verify_pub(UCtx *c) {
  for (int i = 0; i < c->ntps; i++)
    if (!ut_verify_as_parameter(c, c->tps[i])) return 0;
  int total = 2 * c->nmap + c->nextra;
  Ast **args = arena_alloc(sizeof(Ast *) * (total ? total : 1));
  int nargs = uc_type_arguments(c, args);
  Ast **set = arena_alloc(sizeof(Ast *) * (nargs ? nargs : 1));
  int nset = 0;
  for (int i = 0; i < nargs; i++) {
    if (!ut_verify_as_argument(c, args[i])) return 0;
    Ast *r = uc_find(c, args[i]);
    int seen = 0;
    for (int j = 0; j < nset; j++) if (set[j] == r) { seen = 1; break; }
    if (!seen) set[nset++] = r;
  }
  return nset == nargs;
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
    if (!t->u.typeVariable.protocol) return 0;
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
    int nsig = 0;
    for (int k = 0; k < self->u.protocolDecl.signatures.n; k++)
      if (strcmp(self->u.protocolDecl.signatures.v[k]->u.func.name, os->u.func.name) == 0) nsig++;
    if (!nsig) return 0;
    /* the overload list is the protocol's own signatures with this name, in order */
    AstList named = {0};
    for (int k = 0; k < self->u.protocolDecl.signatures.n; k++) {
      Ast *s = self->u.protocolDecl.signatures.v[k];
      if (strcmp(s->u.func.name, os->u.func.name) == 0) list_push(&named, s);
    }
    Ast **pts = arena_alloc(sizeof(Ast *) * (os->u.func.parameters.n ? os->u.func.parameters.n : 1));
    for (int k = 0; k < os->u.func.parameters.n; k++) pts[k] = os->u.func.parameters.v[k]->u.named.type;
    Overload o = resolve_overload(&named, NULL, 0, pts, os->u.func.parameters.n, func_ret_for_overload(os), 0);
    if (!o.func) return 0;
    Ast *sub = subst_unif(o.func->u.func.returnType, o.func->u.func.typeParameters.v, o.func->u.func.typeParameters.n, o.ctx);
    if (!ut_equals(sub, os->u.func.returnType)) return 0;
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
    if (!o.func) return 0;
    Ast *sub = subst_unif(o.func->u.func.returnType, o.func->u.func.typeParameters.v, o.func->u.func.typeParameters.n, o.ctx);
    if (!ut_equals(sub, sig->u.func.returnType)) return 0;
  }
  return 1;
}

/* ---- unify harness (--unify): the first 60 types in a fixed order, one character per ordered pair:
 * bit 0 is Node.unify, bit 1 is Node.equals. Order: struct fields (statement order), function
 * parameter and return types (functions map order), protocol signature types (statement order). */
typedef struct { Ast **out; int n; int cap; } TypeList;
/* the types the run 1a harness covers: no array sizes, constexpr parameters or literals (their
 * unification needs the literal types, which are not ported yet) */
static int ut_plain(const Ast *t) {
  switch (t->tag) {
  case A_TYPEREF:
    for (int i = 0; i < t->u.typeRef.typeArguments.n; i++)
      if (!ut_plain(t->u.typeRef.typeArguments.v[i])) return 0;
    return 1;
  case A_PTRTYPE:
  case A_ARRAYREFTYPE:
    return ut_plain(t->u.ref.elementType);
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
static void unify_harness(Program *p, Buf *b) {
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
  char head[32];
  snprintf(head, sizeof head, "types %d\n", tl.n);
  buf_str(b, head);
  for (int i = 0; i < tl.n; i++) {
    for (int j = 0; j < tl.n; j++) {
      UCtx *c = uc_new(NULL, 0);
      int u = ut_unify(c, tl.out[i], tl.out[j]);
      int e = ut_equals(tl.out[i], tl.out[j]) != NULL;
      char one[2] = {(char)('0' + (u ? 1 : 0) + (e ? 2 : 0)), 0};
      buf_str(b, one);
    }
    buf_str(b, "\n");
  }
}

#endif
