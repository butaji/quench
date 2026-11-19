/* WSL PropertyResolver.js (resolveProperties): lowers DotExpression and IndexExpression to calls of
 * getters (operator.f, operator[]), setters and anders (operator&.f, operator&[]), and turns
 * assignments and read-modify-writes whose lvalue is a property access into lvalue emulation. A C
 * port of PropertyResolver.js, PropertyAccessExpression.js (rewriteAfterCloning, emitGet, emitSet),
 * NormalUsePropertyResolver.js and the Rewriter's expression cases (wsl-synth.h's Rewriter covers types
 * only). The walks follow the JS order, because the anonymous variables it makes are created in that
 * order. Anonymous variables are named "anonVar" in the output; the JS counter is not reproduced, so an
 * error message that prints one can differ in its index. */
#ifndef WSL_PROPS_H
#define WSL_PROPS_H

#include "wsl-check.h"

static Ast *prop_base(const Ast *n) { return n->tag == A_DOT ? n->u.dot.base : n->u.index.base; }
static void prop_set_base(Ast *n, Ast *b) { if (n->tag == A_DOT) n->u.dot.base = b; else n->u.index.base = b; }

static Ast *shallow_copy(const Ast *n) {
  Ast *r = arena_alloc(sizeof(Ast));
  memcpy(r, n, sizeof(Ast));
  return r;
}

/* ---- node makers (the JS constructors; origin from the source node) ---- */
static Ast *anon_new(const Ast *src, Ast *type) {   /* new AnonymousVariable(origin, type) */
  Ast *a = mk_from(A_ANONVAR, src);
  a->type = type;
  return a;
}
static Ast *vref_of(Ast *v) {   /* VariableRef.wrap(anonymous variable) */
  Ast *r = mk_from(A_VARIABLEREF, v);
  r->u.varRef.name = "anonVar";
  r->u.varRef.variable = v;
  return r;
}
static Ast *mk_assign(const Ast *src, Ast *lhs, Ast *rhs, Ast *type) {
  Ast *a = mk_from(A_ASSIGN, src);
  a->u.assign.lhs = lhs;
  a->u.assign.rhs = rhs;
  a->type = type;
  return a;
}
static Ast *mk_deref(const Ast *src, Ast *ptr, const char *space) {
  Ast *a = mk_from(A_DEREF, src);
  a->u.deref.ptr = ptr;
  a->u.deref.addrSpace = space;
  return a;
}
static Ast *mk_makeptr(const Ast *src, Ast *lv) {
  Ast *a = mk_from(A_MAKEPTR, src);
  a->u.lvalueRef.lValue = lv;
  return a;
}
static Ast *mk_comma(const Ast *src, Ast **items, int n) {
  Ast *a = mk_from(A_COMMA, src);
  for (int i = 0; i < n; i++) list_push(&a->u.comma.list, items[i]);
  return a;
}
static Ast *mk_rmw(const Ast *src, Ast *lv, Ast *type) {   /* new ReadModifyWriteExpression(origin, lv, type) */
  Ast *a = mk_from(A_RMW, src);
  a->u.rmw.lValue = lv;
  a->u.rmw.oldVar = anon_new(src, type);
  a->u.rmw.newVar = anon_new(src, type);
  return a;
}

/* ---- the Rewriter of the expression cases. lower=1: NormalUsePropertyResolver (each DotExpression and
 * IndexExpression is rewriteAfterCloning'd as it is copied); lower=0: the plain Rewriter. ---- */
#define NU_MAX 256
typedef struct { Ast *from[NU_MAX]; Ast *to[NU_MAX]; int n; int lower; } NuCx;
static NuCx *nu_new(int lower) {
  NuCx *c = arena_alloc(sizeof(NuCx));
  memset(c, 0, sizeof *c);
  c->lower = lower;
  return c;
}
static void nu_set(NuCx *c, Ast *from, Ast *to) {
  if (c->n >= NU_MAX) fatal_unsupported("Rewriter mapping overflow", from);
  c->from[c->n] = from;
  c->to[c->n++] = to;
}
static Ast *nu_map(const NuCx *c, Ast *v) {   /* _getMapping: the latest mapping of v */
  for (int i = c->n - 1; i >= 0; i--) if (c->from[i] == v) return c->to[i];
  return v;
}
static Ast *nu_visit(NuCx *c, Ast *n);
static AstList nu_list(NuCx *c, const AstList *src) {
  AstList d = {NULL, 0, 0};
  for (int i = 0; i < src->n; i++) list_push(&d, nu_visit(c, src->v[i]));
  return d;
}
static Ast *nu_anon(NuCx *c, Ast *v) {   /* Rewriter.visitAnonymousVariable */
  Ast *a = mk_from(A_ANONVAR, v);
  a->type = v->type;
  nu_set(c, v, a);
  return a;
}

/* ReadModifyWriteExpression / PropertyAccessExpression.updateCalls: the calls take the base as argument 0 */
static void prop_update_calls(Ast *r) {
  PropChk *p = prop_chk(r);
  Ast *base = prop_base(r);
  if (p->callGet) p->callGet->u.call.argumentList.v[0] = base;
  if (p->callAnd) p->callAnd->u.call.argumentList.v[0] = arg_for_and(r, p->baseType, base);
  if (p->callSet) p->callSet->u.call.argumentList.v[0] = base;
}

/* PropertyAccessExpression.rewriteAfterCloning: the access as an expression over the getter, ander and setter calls */
static Ast *prop_rewrite_after_cloning(Ast *n) {
  PropChk *p = prop_chk(n);
  Ast *base = prop_base(n);
  if ((is_lvalue(base) || is_ref_t(p->baseType)) && p->callAnd)
    return mk_deref(n, p->callAnd, p->callAnd->u.call.resultType->u.ref.addressSpace);
  if (p->callGet) return p->callGet;
  if (!p->callAnd)
    chk_error(cat(4, origin_string(n), ": Property access without getter and ander: ", js_tostr(n), ""));
  Ast *anon = anon_new(n, p->baseType);
  p->callAnd->u.call.argumentList.v[0] = arg_for_and(n, p->baseType, vref_of(anon));
  Ast *items[3] = {
    anon,
    mk_assign(n, vref_of(anon), base, p->baseType),
    mk_deref(n, p->callAnd, p->callAnd->u.call.resultType->u.ref.addressSpace)
  };
  return mk_comma(n, items, 3);
}

static Ast *nu_property(NuCx *c, Ast *n) {
  Ast *r = shallow_copy(n);
  const PropChk *o = prop_chk(n);
  prop_set_base(r, nu_visit(c, prop_base(n)));
  if (n->tag == A_INDEX) r->u.index.index = nu_visit(c, n->u.index.index);
  PropChk *p = prop_chk(r);
  p->callGet = nu_visit(c, o->callGet);
  p->callAnd = nu_visit(c, o->callAnd);
  p->callSet = nu_visit(c, o->callSet);
  prop_update_calls(r);
  return c->lower ? prop_rewrite_after_cloning(r) : r;
}

static Ast *nu_visit(NuCx *c, Ast *n) {
  if (!n) return NULL;
  switch (n->tag) {
  case A_VARIABLEREF: {
    Ast *r = shallow_copy(n);
    r->u.varRef.variable = nu_map(c, n->u.varRef.variable);
    return r;
  }
  case A_BOOLLITERAL:
    return n;
  case A_NULLLITERAL: {
    Ast *r = lit_expr_new(A_NULLLITERAL, n->origin);
    r->native = n->native;
    r->type->type = nu_visit(c, n->type->type);   /* Rewriter.visitNullType: result.type = Node.visit(node.type, this) */
    return r;
  }
  case A_INTLITERAL: case A_UINTLITERAL: {
    /* Rewriter.visitGenericLiteral: an IntLiteral with the same value and a copy of the literal type */
    if (!n->type) fatal_unsupported("literal without a type", n);
    Ast *t = mk_from(A_INTLITERAL, n);
    if (is_lit_type_tag(n->type->tag)) {
      t->type = lit_type_new(n->type->tag, t);
      if (n->type->type) t->type->type = nu_visit(c, n->type->type);   /* visitGenericLiteralType: result.type = Node.visit(node.type, this) */
    } else t->type = nu_visit(c, n->type);   /* a typeref-wrapped literal (withType): an ordinary type */
    t->u.intLit.value = n->u.intLit.value;
    t->u.intLit.dbl = n->u.intLit.dbl;
    t->u.intLit.isDbl = n->u.intLit.isDbl;
    t->u.intLit.preferred = rw_plain(literal_preferred(n));
    return t;
  }
  case A_FLOATLITERAL: case A_DOUBLELITERAL: {
    /* Rewriter.visitGenericLiteral: the JS copy is an IntLiteral holding the double (a FIXME there). The C
     * IntLiteral has the isDbl field for it, so the copy keeps the exact double value like the JS. */
    if (!n->type) fatal_unsupported("literal without a type", n);
    Ast *t = mk_from(A_INTLITERAL, n);
    if (is_lit_type_tag(n->type->tag)) {
      t->type = lit_type_new(n->type->tag, t);
      if (n->type->type) t->type->type = nu_visit(c, n->type->type);
    } else t->type = nu_visit(c, n->type);
    t->u.intLit.isDbl = 1;
    t->u.intLit.dbl = n->u.fltLit.value;
    t->u.intLit.value = 0;
    t->u.intLit.preferred = rw_plain(literal_preferred(n));
    return t;
  }
  case A_NATIVETYPE: case A_STRUCTTYPE: case A_TYPEDEF: case A_ENUMTYPE: case A_TYPEVARIABLE:
  case A_CONSTEXPRTYPEPARAM: case A_PROTOCOLDECL: case A_FUNCDEF: case A_NATIVEFUNC: case A_NATIVEFUNCINST:
  case A_INTLITERALTYPE: case A_UINTLITERALTYPE: case A_FLOATLITERALTYPE: case A_DOUBLELITERALTYPE: case A_NULLTYPE:
    return n;   /* Rewriter's identity visits (visitNativeType, visitStructType, visitTypeVariable, ...) */
  case A_TYPEREF: {   /* Rewriter.visitTypeRef: a new TypeRef, its arguments and its target visited */
    Ast *r = mk_from(A_TYPEREF, n);
    r->u.typeRef.name = n->u.typeRef.name;
    r->u.typeRef.typeArguments = nu_list(c, &n->u.typeRef.typeArguments);
    r->u.typeRef.type = nu_visit(c, n->u.typeRef.type);
    return r;
  }
  case A_ENUMLITERAL: {
    Ast *r = mk_from(A_ENUMLITERAL, n);
    r->u.enumLit.member = n->u.enumLit.member;
    return r;
  }
  case A_IDENTITY: {
    Ast *r = shallow_copy(n);
    r->u.identity.target = nu_visit(c, n->u.identity.target);
    return r;
  }
  case A_CALL: {
    Ast *r = shallow_copy(n);
    r->u.call.argumentList = nu_list(c, &n->u.call.argumentList);
    return r;
  }
  case A_DOT: case A_INDEX:
    return nu_property(c, n);
  case A_DEREF: {
    Ast *r = shallow_copy(n);
    r->u.deref.ptr = nu_visit(c, n->u.deref.ptr);
    return r;
  }
  case A_MAKEPTR: case A_MAKEARRAYREF: case A_CONVERTPTR: {
    Ast *r = shallow_copy(n);
    r->u.lvalueRef.lValue = nu_visit(c, n->u.lvalueRef.lValue);
    return r;
  }
  case A_LOGICALNOT: {
    Ast *r = shallow_copy(n);
    r->u.notExpr.operand = nu_visit(c, n->u.notExpr.operand);
    return r;
  }
  case A_LOGICAL: {
    Ast *r = shallow_copy(n);
    r->u.logical.left = nu_visit(c, n->u.logical.left);
    r->u.logical.right = nu_visit(c, n->u.logical.right);
    return r;
  }
  case A_ASSIGN: {
    Ast *r = shallow_copy(n);
    r->u.assign.lhs = nu_visit(c, n->u.assign.lhs);
    r->u.assign.rhs = nu_visit(c, n->u.assign.rhs);
    return r;
  }
  case A_RMW: {
    Ast *r = shallow_copy(n);
    r->u.rmw.lValue = nu_visit(c, n->u.rmw.lValue);
    r->u.rmw.oldVar = nu_anon(c, n->u.rmw.oldVar);
    r->u.rmw.newVar = nu_anon(c, n->u.rmw.newVar);
    r->u.rmw.newValueExp = nu_visit(c, n->u.rmw.newValueExp);
    r->u.rmw.resultExp = nu_visit(c, n->u.rmw.resultExp);
    return r;
  }
  case A_COMMA: {
    Ast *r = shallow_copy(n);
    r->u.comma.list = nu_list(c, &n->u.comma.list);
    return r;
  }
  case A_ANONVAR:
    return nu_anon(c, n);
  default:
    fatal_unsupported("NormalUsePropertyResolver/Rewriter", n);
  }
  return NULL;
}

/* PropertyAccessExpression.emitGet(base) and emitSet(base, value) */
static Ast *emit_get(Ast *lv, Ast *base) {
  Ast *r = nu_visit(nu_new(0), lv);
  prop_set_base(r, base);
  prop_update_calls(r);
  return prop_rewrite_after_cloning(r);
}
static Ast *emit_set(Ast *lv, Ast *base, Ast *value) {
  Ast *r = nu_visit(nu_new(0), lv);
  prop_set_base(r, base);
  prop_update_calls(r);
  PropChk *p = prop_chk(r);
  if (!p->callSet)
    wtype_error(origin_string(lv), cat(2, "Cannot emit set because: ", prop_chk(lv)->errSet));
  AstList *args = &p->callSet->u.call.argumentList;
  args->v[args->n - 1] = value;
  return p->callSet;
}

/* ---- PropertyResolver: the visitor ---- */
static void rp_visit(Ast *n);
static void rp_child(void *cx, Ast *c) { (void)cx; rp_visit(c); }

/* _visitRValuesWithinLValue: the RValueFinder walk. The index and dereferenced pointers are visited by the
 * PropertyResolver itself; the struct and array parts are only walked. */
static void rvf_visit(Ast *n) {
  switch (n->tag) {
  case A_DOT: rvf_visit(n->u.dot.base); return;
  case A_INDEX: rvf_visit(n->u.index.base); rp_visit(n->u.index.index); return;
  case A_VARIABLEREF: return;
  case A_DEREF: rp_visit(n->u.deref.ptr); return;
  case A_IDENTITY: rvf_visit(n->u.identity.target); return;
  case A_MAKEARRAYREF: rp_visit(n->u.lvalueRef.lValue); return;
  default:
    chk_error(cat(3, "No visit function for ", ast_class_name(n), " in RValueFinder"));
  }
}

/* _visitPropertyAccess */
static void rp_property(Ast *n) {
  Ast *nn = nu_visit(nu_new(1), n);
  rp_visit(nn);
  ast_become(n, nn);
}

/* _handleReadModifyWrite */
static void rmw_handle(Ast *n) {
  Ast *type = var_type(n->u.rmw.oldVar);
  if (!type) chk_error("Null type");
  Ast *simple = nu_visit(nu_new(1), n->u.rmw.lValue);
  if (is_lvalue(simple)) {
    const char *space = addr_space(simple);
    if (!space)
      chk_error(cat(4, origin_string(n), ": LHS without address space: ", js_tostr(simple), ""));
    Ast *ptrType = mk_ptr_type(n, space, type);
    Ast *ptrVar = anon_new(n, ptrType);
    Ast *items[8] = {
      n->u.rmw.oldVar, n->u.rmw.newVar, ptrVar,
      mk_assign(n, vref_of(ptrVar), mk_makeptr(n, simple), ptrType),
      mk_assign(n, vref_of(n->u.rmw.oldVar), mk_deref(n, vref_of(ptrVar), space), type),
      mk_assign(n, vref_of(n->u.rmw.newVar), n->u.rmw.newValueExp, type),
      mk_assign(n, mk_deref(n, vref_of(ptrVar), space), vref_of(n->u.rmw.newVar), type),
      n->u.rmw.resultExp
    };
    ast_become(n, mk_comma(n, items, 8));
    return;
  }
  Ast *lv = n->u.rmw.lValue;
  if (lv->tag != A_DOT && lv->tag != A_INDEX)
    chk_error(cat(4, origin_string(n), ": read-modify-write of a non-property lvalue: ", js_tostr(lv), ""));
  Ast *result = mk_rmw(n, prop_base(lv), prop_chk(lv)->baseType);
  Ast *getv = emit_get(lv, vref_of(result->u.rmw.oldVar));
  Ast *setv = emit_set(lv, vref_of(result->u.rmw.oldVar), vref_of(n->u.rmw.newVar));
  Ast *items[5] = {
    n->u.rmw.oldVar, n->u.rmw.newVar,
    mk_assign(n, vref_of(n->u.rmw.oldVar), getv, type),
    mk_assign(n, vref_of(n->u.rmw.newVar), n->u.rmw.newValueExp, type),
    setv
  };
  result->u.rmw.newValueExp = mk_comma(n, items, 5);
  result->u.rmw.resultExp = vref_of(n->u.rmw.newVar);
  rmw_handle(result);
  ast_become(n, result);
}

/* visitAssignment */
static void rp_assign(Ast *n) {
  Ast *lhs = n->u.assign.lhs;
  rvf_visit(lhs);
  rp_visit(n->u.assign.rhs);
  Ast *simple = nu_visit(nu_new(1), lhs);
  if (is_lvalue(simple)) {
    ast_become(lhs, simple);
    return;
  }
  if (lhs->tag != A_DOT && lhs->tag != A_INDEX)
    chk_error(cat(2, "Unexpected lhs type: ", ast_class_name(lhs)));
  Ast *rhs = n->u.assign.rhs;
  Ast *type = n->type;
  Ast *result = mk_rmw(n, prop_base(lhs), prop_chk(lhs)->baseType);
  Ast *resultVar = anon_new(n, type);
  Ast *items[3] = {
    resultVar,
    mk_assign(n, vref_of(resultVar), rhs, type),
    emit_set(lhs, vref_of(result->u.rmw.oldVar), vref_of(resultVar))
  };
  result->u.rmw.newValueExp = mk_comma(n, items, 3);
  result->u.rmw.resultExp = vref_of(resultVar);
  rmw_handle(result);
  ast_become(n, result);
}

/* visitReadModifyWriteExpression */
static void rp_rmw(Ast *n) {
  rp_visit(n->u.rmw.newValueExp);
  rp_visit(n->u.rmw.resultExp);
  rvf_visit(n->u.rmw.lValue);
  rmw_handle(n);
}

static void rp_visit(Ast *n) {
  if (!n) return;
  switch (n->tag) {
  case A_DOT: case A_INDEX: rp_property(n); return;
  case A_RMW: rp_rmw(n); return;
  case A_ASSIGN: rp_assign(n); return;
  case A_MAKEPTR: case A_MAKEARRAYREF:
    visit_children(n, rp_child, NULL);
    if (!is_lvalue(n->u.lvalueRef.lValue))
      wtype_error(origin_string(n), cat(2, "Not an lvalue: ", js_tostr(n->u.lvalueRef.lValue)));
    return;
  default:
    visit_children(n, rp_child, NULL);
  }
}

/* resolveProperties(program) */
static void resolve_properties(Program *p) {
  for (int i = 0; i < p->statements.n; i++) rp_visit(p->statements.v[i]);
}

#endif
