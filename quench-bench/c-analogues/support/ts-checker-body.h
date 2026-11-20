/*
 * Checker port, bodies: checkSourceElement over the statements and declarations of a source file, and
 * checkExpression for the ported expression forms (TypeScript 5.9.2 checker.ts; lib/typescript.js line
 * numbers in comments). Every expression type is cached on its node (links.resolvedType of checkExpressionCached),
 * so each expression is typed once, as in node.
 *
 * PORTED:
 *   - checkSourceElement: the statements (variable statements, expression, if, do, while, for, for-in, for-of,
 *     return, throw, switch, try, block, labeled), function-like bodies (declarations, methods, constructors,
 *     accessors, function and arrow expressions with an expression body), class members (property initializers,
 *     parameter initializers, static blocks), module blocks and export assignments;
 *   - the assignability of an initializer to the declared type of a variable, parameter or property (2322 at the
 *     name), of a return expression to the annotated return type (2322 at the return statement), of an arrow body
 *     to its annotated return type (2322 at the body), and of the right side of `=` to a named target (2322 at the
 *     left side). An object or array literal source is elaborated (cElaborateObjectLiteral) or not reported;
 *   - checkExpression: literals (fresh literal types, 1 with its negative), identifiers (declared type, then the
 *     flow type: the flow graph of the binder is walked back from the reference, and any assignment or condition
 *     that could narrow it, with a non-union declared type or with a union, is unported), parenthesized, as,
 *     non-null, this, typeof, void, delete, comparison and logical-equality operators (boolean), arithmetic (number),
 *     `=` (the right type), the comma operator, conditional expressions (subtype union), property and element
 *     access with a literal or unique symbol key on object and any types, calls and new of a single non-generic
 *     signature (the arguments are related to the parameters; the result is the return type), object literals
 *     without a contextual type (anonymous type of the literal's symbol, widened property types), array literals
 *     without a contextual type (Array of the subtype-reduced union, with the ArrayLiteral clone), function and
 *     arrow expressions without a contextual type (the anonymous function type; the body is checked);
 *   - the widening of the literal types of mutable locations (getWidenedLiteralType: fresh literals only) and the
 *     type of an unannotated variable, property or parameter from its initializer.
 * NOT PORTED (any of these is cUnported, whose type is the error type, so it never reports): contextual typing
 * (any expression with a contextual type other than none is unported: arguments of generic or overloaded calls,
 * arrow arguments, array literal elements of a contextual type, object literal properties of a contextual type),
 * overload resolution and generic signatures (inference), optional chains, spreads, template expressions with
 * substitutions, tagged templates, classes and regular expressions as expressions, await and yield, super,
 * compound assignments, the logical operators && || ??, arithmetic on bigint, the subtype-based narrowing of
 * unions by conditions (a condition that mentions a reference makes its type unported), comparability errors
 * (2367, 2678) and the excess property check of object literals.
 */
#ifndef TS_CHECKER_BODY_H
#define TS_CHECKER_BODY_H

#include "ts-checker-expr.h"

/* ---- tree helpers ---- */
static TsNode *cSkipParens(TsNode *e) {
  while (e && e->kind == TSK_ParenthesizedExpression) e = e->expression;
  return e;
}

static bool cIsFunctionLikeKind(uint16_t k) {
  return k == TSK_FunctionDeclaration || k == TSK_FunctionExpression || k == TSK_ArrowFunction || k == TSK_MethodDeclaration ||
         k == TSK_Constructor || k == TSK_GetAccessor || k == TSK_SetAccessor;
}

/* getControlFlowContainer: the nearest function-like node, module block, source file or property declaration */
static TsNode *cFlowContainerOf(TsNode *n) {
  for (TsNode *p = n->parent; p; p = p->parent) {
    if (cIsFunctionLikeKind(p->kind) || p->kind == TSK_SourceFile || p->kind == TSK_ModuleBlock ||
        p->kind == TSK_PropertyDeclaration || p->kind == TSK_ClassStaticBlockDeclaration)
      return p;
  }
  return NULL;
}

/* does a node or any of its descendants name the text (an identifier, a property name included) */
static bool cMentionsText(TsNode *n, const char *text, int len) {
  if (!n) return false;
  if (n->kind == TSK_Identifier && n->textLen == len && memcmp(n->text, text, (size_t)len) == 0) return true;
  BNodeVec nv = { NULL, 0, 0 };
  bChildrenSorted(n, &nv);
  bool r = false;
  for (int i = 0; i < nv.n && !r; i++) r = cMentionsText(nv.v[i], text, len);
  free(nv.v);
  return r;
}

/* the same reference: identifiers of one symbol, this, and property or element accesses of the same references */
static bool cSameChain(TsNode *a, TsNode *b) {
  a = cSkipParens(a);
  b = cSkipParens(b);
  if (!a || !b) return false;
  if (a->kind == TSK_Identifier && b->kind == TSK_Identifier) {
    if (a->textLen != b->textLen || memcmp(a->text, b->text, (size_t)a->textLen) != 0) return false;
    return cResolveValueName(a, bnEscapeText(a->text, a->textLen)) == cResolveValueName(b, bnEscapeText(b->text, b->textLen));
  }
  if (a->kind == TSK_ThisKeyword && b->kind == TSK_ThisKeyword) return true;
  if (a->kind == TSK_PropertyAccessExpression && b->kind == TSK_PropertyAccessExpression) {
    if (!a->name || !b->name || a->name->textLen != b->name->textLen ||
        memcmp(a->name->text, b->name->text, (size_t)a->name->textLen) != 0)
      return false;
    return cSameChain(a->expression, b->expression);
  }
  if (a->kind == TSK_ElementAccessExpression && b->kind == TSK_ElementAccessExpression) {
    TsNode *ka = cSkipParens(a->argumentExpression), *kb = cSkipParens(b->argumentExpression);
    if (!ka || !kb || ka->kind != kb->kind || (ka->kind != TSK_StringLiteral && ka->kind != TSK_NumericLiteral)) return false;
    if (ka->textLen != kb->textLen || memcmp(ka->text, kb->text, (size_t)ka->textLen) != 0) return false;
    return cSameChain(a->expression, b->expression);
  }
  return false;
}

/* ---- widening (getWidenedLiteralType, 5.9.2): only fresh literal types widen ---- */
static bool cIsFreshLiteral(TsType *t) {
  if (t->flags & TF_BooleanLiteral) return true;  /* trueType and falseType are the fresh objects */
  return (t->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral)) && t->litFresh == t;
}

static TsType *cWidenLiteralType(TsType *t) {
  if (t == cBooleanType || t == cErrorType) return t;
  if (t->flags & TF_Union) {
    bool any = false;
    for (int i = 0; i < t->ntypes; i++) if (cIsFreshLiteral(t->types[i])) any = true;
    if (!any) return t;
    TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)t->ntypes);
    for (int i = 0; i < t->ntypes; i++) v[i] = cWidenLiteralType(t->types[i]);
    return cGetUnionType(v, t->ntypes);
  }
  if (t->flags & TF_EnumLiteral) return cUnported("widening of an enum literal");
  if (!cIsFreshLiteral(t)) return t;
  if (t->flags & TF_StringLiteral) return cStringType;
  if (t->flags & TF_NumberLiteral) return cNumberType;
  if (t->flags & TF_BooleanLiteral) return cBooleanType;
  if (t->flags & TF_BigIntLiteral) return cBigIntType;
  return t;
}

/* addOptionality: T | undefined (under strictNullChecks), as the declared types of optional members are built */
static TsType *cAddUndefined(TsType *t) {
  if (t == cErrorType) return t;
  TsType *u = cNewType(TF_Union);
  u->types = ts_arena_alloc(sizeof(TsType *) * 2);
  u->types[0] = t;
  u->types[1] = cUndefinedType;
  u->ntypes = 2;
  return u;
}

/* ---- flow types of references (getFlowTypeOfReference, conservative) ----
 * The walk goes back from the reference's flow node. It succeeds (the declared type applies on every path) when no
 * node on the way can narrow the reference: an assignment to the reference resets it to the declared type (a
 * non-union declared type only); a condition, a switch clause or an argument of a statement call that mentions the
 * reference makes it unported. */
typedef struct {
  TsNode *node;          /* the reference: an identifier or an access chain */
  const char *text;      /* the name the references of the walk are compared by (conditions are scanned for it) */
  int len;
  TsType *declared;
  bool nonUnion;         /* the declared type is not a union: an assignment resets to it */
  bool outer;            /* the declaration is in an enclosing function (its narrowing is not walked) */
  bool startInit;        /* the reference is initialized at the start of its container (parameter, assigned, ...) */
} CFlowRef;

static uint32_t cFlowStamp;
typedef struct { TsFlow *target; TsFlow **ants; int n; } CRedOverride;
static CRedOverride cRedOverrides[32];
static int cRedOverrideN;

/* the target of an assignment flow node: the name of a declaration, the left side of `=`, the operand of ++ or -- */
static TsNode *cAssignTarget(TsNode *n) {
  switch (n->kind) {
    case TSK_VariableDeclaration: case TSK_BindingElement: case TSK_Parameter: return n->name;
    case TSK_BinaryExpression: return n->left;
    case TSK_PrefixUnaryExpression: case TSK_PostfixUnaryExpression: return n->operand;
    default: return n;
  }
}

/* containsMatchingReference: the target is the reference or an access chain that contains it */
static bool cAssignMatches(TsNode *target, CFlowRef *r) {
  TsNode *t = cSkipParens(target);
  for (;;) {
    if (cSameChain(t, r->node)) return true;
    if (!t || (t->kind != TSK_PropertyAccessExpression && t->kind != TSK_ElementAccessExpression)) return false;
    t = cSkipParens(t->expression);
  }
}

static bool cOuterNarrowingSafe(TsType *d) {
  if (d->flags & TF_Union) return false;
  return (d->flags & (TF_Object | TF_String | TF_Number | TF_BigInt | TF_ESSymbol | TF_UniqueESSymbol | TF_NonPrimitive |
                      TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral | TF_Undefined | TF_Null | TF_Void)) != 0;
}

static bool cFlowRefWalk(TsFlow *f, CFlowRef *r) {
  for (;;) {
    if (!f) return false;
    if (f->visit == cFlowStamp) return true;
    f->visit = cFlowStamp;
    uint32_t fl = f->flags;
    if (fl & FF_Unreachable) return false;
    if (fl & FF_Start) {
      if (r->outer) return cOuterNarrowingSafe(r->declared);
      return r->startInit;
    }
    if (fl & FF_ReduceLabel) {
      if (cRedOverrideN == 32) return false;
      cRedOverrides[cRedOverrideN].target = f->target;
      cRedOverrides[cRedOverrideN].ants = f->redAnts;
      cRedOverrides[cRedOverrideN].n = f->nRedAnts;
      cRedOverrideN++;
      bool ok = cFlowRefWalk(f->ante, r);
      cRedOverrideN--;
      return ok;
    }
    if (fl & (FF_BranchLabel | FF_LoopLabel)) {
      TsFlow **ants = f->labelAnts;
      int n = f->nLabelAnts;
      for (int i = cRedOverrideN - 1; i >= 0; i--)
        if (cRedOverrides[i].target == f) { ants = cRedOverrides[i].ants; n = cRedOverrides[i].n; break; }
      for (int i = 0; i < n; i++)
        if (!cFlowRefWalk(ants[i], r)) return false;
      return true;
    }
    if (fl & FF_Assignment) {
      TsNode *t = cAssignTarget(f->node);
      if (t && cAssignMatches(t, r)) return r->nonUnion;
      if (t && t->kind != TSK_Identifier && t->kind != TSK_PropertyAccessExpression && t->kind != TSK_ElementAccessExpression &&
          cMentionsText(t, r->text, r->len))
        return false;
      f = f->ante;
      continue;
    }
    if (fl & (FF_TrueCondition | FF_FalseCondition)) {
      if (cMentionsText(f->node, r->text, r->len)) return false;
      f = f->ante;
      continue;
    }
    if (fl & FF_SwitchClause) {
      if (cMentionsText(f->node->expression, r->text, r->len)) return false;
      f = f->ante;
      continue;
    }
    if (fl & FF_Call) {
      if (f->node->arguments)
        for (int i = 0; i < f->node->arguments->n; i++)
          if (cMentionsText(f->node->arguments->v[i], r->text, r->len)) return false;
      f = f->ante;
      continue;
    }
    f = f->ante;   /* assignments of array mutations and the other flow kinds do not narrow a declared type */
  }
}

/* getFlowTypeOfReference: the declared type, or the error type when the flow may narrow it */
static TsType *cFlowTypeOfReference(TsNode *ref, CFlowRef *r) {
  TsType *d = r->declared;
  if (!ref->flowNode || (d->flags & (TF_Undefined | TF_Null | TF_Void | TF_Never))) return d;
  cFlowStamp++;
  r->nonUnion = !(d->flags & TF_Union);
  if (cFlowRefWalk(ref->flowNode, r)) return d;
  return cUnported("flow narrowing of a reference");
}

/* the declaration of a symbol (the first one for a merged symbol) */
static TsNode *cFirstDeclOf(TsSymbol *s) { return s->valueDeclaration ? s->valueDeclaration : (s->ndecls ? s->decls[0] : NULL); }

/* the declared type of a reference to a variable: an annotation, else the initializer (unported when the flow
 * gives the declared type of a union with a generic constraint) */
static TsType *cReferenceDeclaredType(TsNode *ref, TsSymbol *s, bool *outer, bool *startInit) {
  TsType *d = cTypeOfSymbol(s);
  TsNode *decl = cFirstDeclOf(s);
  *outer = false;
  *startInit = true;
  if (!decl || d == cErrorType) return d;
  TsNode *refC = cFlowContainerOf(ref), *declC = cFlowContainerOf(decl);
  if (decl->kind == TSK_Parameter) {
    /* removeOptionalityFromDeclaredType: an optional parameter is read without its undefined */
    if ((decl->questionToken || decl->initializer) && decl->type) d = cTypeFromNode(decl->type);
  }
  if (refC != declC && decl->kind != TSK_ImportSpecifier) *outer = true;
  if (decl->kind == TSK_VariableDeclaration) {
    *startInit = decl->initializer || decl->exclamationToken || *outer || (d->flags & (TF_Any | TF_Unknown | TF_Void)) ||
                 (decl->flags & TNF_Ambient);
  }
  if (decl->kind == TSK_BindingElement) *startInit = true;
  return d;
}

/* the type of an identifier reference (checkIdentifier): the flow type of a variable, else the type of the symbol */
static TsType *cIdentifierType(TsNode *e) {
  BName nm = bnEscapeText(e->text, e->textLen);
  TsSymbol *s = cResolveValueName(e, nm);
  if (!s) {
    if (e->textLen == 9 && memcmp(e->text, "undefined", 9) == 0) return cUndefinedType;
    return cUnported("unresolved value");
  }
  if (s == cUndefinedSymbol) return cUndefinedType;
  if (!(s->flags & (SF_Variable | SF_Alias))) return cTypeOfSymbol(s);
  bool outer, startInit;
  TsType *d = cReferenceDeclaredType(e, s, &outer, &startInit);
  if (d == cErrorType) return d;
  if (d->flags & TF_TypeParameter) {
    TsType *c = cConstraintOf(d);
    if (c && (c->flags & TF_Union)) return cUnported("reference of a type parameter with a union constraint");
  }
  CFlowRef r = { e, e->text, e->textLen, d, false, outer, startInit };
  return cFlowTypeOfReference(e, &r);
}

/* the type of a property access or element access reference, narrowed by the flow (the declared type of the member) */
static TsType *cAccessReferenceFlow(TsNode *e, TsType *declared, bool startInit) {
  if (declared == cErrorType) return declared;
  const char *text = NULL;
  int len = 0;
  if (e->kind == TSK_PropertyAccessExpression && e->name) { text = e->name->text; len = e->name->textLen; }
  else {
    TsNode *k = cSkipParens(e->argumentExpression);
    if (!k || (k->kind != TSK_StringLiteral && k->kind != TSK_NumericLiteral)) return cUnported("element access reference");
    text = k->text;
    len = k->textLen;
  }
  CFlowRef r = { e, text, len, declared, false, false, startInit };
  return cFlowTypeOfReference(e, &r);
}

/* ObjectFlags of an array literal type (createArrayLiteralType): the ArrayLiteral flag of a clone */
#define OF_ArrayLiteral 0x2000

/* ---- contextual types (getContextualType): NULL when there is none, the error type when it is not ported ---- */
static TsType *cContextualType(TsNode *e) {
  TsNode *c = e, *p = e->parent;
  while (p && p->kind == TSK_ParenthesizedExpression) { c = p; p = p->parent; }
  if (!p) return NULL;
  switch (p->kind) {
    case TSK_VariableDeclaration:
      if (p->initializer != c) return cUnported("contextual type of a declaration name");
      return p->type ? cTypeFromNode(p->type) : NULL;
    case TSK_PropertyDeclaration:
      if (p->initializer != c) return cUnported("contextual type of a property");
      return p->type ? cTypeFromNode(p->type) : NULL;
    case TSK_ExpressionStatement: case TSK_IfStatement: case TSK_DoStatement: case TSK_WhileStatement:
    case TSK_ForStatement: case TSK_ThrowStatement: case TSK_CaseClause: case TSK_SwitchStatement:
      return NULL;
    case TSK_ReturnStatement: {
      TsNode *fn = p->parent;
      while (fn && !cIsFunctionLikeKind(fn->kind)) fn = fn->parent;
      if (!fn) return NULL;
      if (fn->type) return fn->asteriskToken || (bModifierFlags(fn) & TMF_Async) ? cUnported("contextual return of a generator or async function") : cTypeFromNode(fn->type);
      return cUnported("contextual return type of an unannotated function");
    }
    case TSK_AsExpression:
      if (p->expression != c) return cUnported("contextual type of an assertion type");
      return cTypeFromNode(p->type);
    default: break;
  }
  return cUnported("contextual type");
}

/* ---- expression types (checkExpression, cached on the node) ---- */
static TsType *cExprWorker(TsNode *e);
static TsType *cCheckNonNullable(TsType *t) {
  if (!(t->flags & TF_Union)) {
    if (t->flags & (TF_Undefined | TF_Null)) return cNeverType;
    return t;
  }
  bool any = false;
  for (int i = 0; i < t->ntypes; i++) if (t->types[i]->flags & (TF_Undefined | TF_Null)) any = true;
  if (!any) return t;
  TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)t->ntypes);
  int n = 0;
  for (int i = 0; i < t->ntypes; i++) if (!(t->types[i]->flags & (TF_Undefined | TF_Null))) v[n++] = t->types[i];
  if (n == 0) return cNeverType;
  return n == 1 ? v[0] : cGetUnionType(v, n);
}

static TsType *cExprType(TsNode *e) {
  if (!e) return cUnported("missing expression");
  if (e->exprState == 2) return e->exprType;
  if (e->exprState == 1) return cUnported("circular expression");
  e->exprState = 1;
  TsType *t = cExprWorker(e);
  e->exprType = t;
  e->exprState = 2;
  return t;
}

/* the type of the expression of a `this` (the class instance type; static members and functions are not ported) */
static TsType *cThisType(TsNode *e) {
  for (TsNode *p = e->parent; p; p = p->parent) {
    if (p->kind == TSK_ArrowFunction) continue;
    if (p->kind == TSK_FunctionExpression || p->kind == TSK_FunctionDeclaration) return cUnported("this in a function");
    if (p->kind == TSK_MethodDeclaration || p->kind == TSK_PropertyDeclaration || p->kind == TSK_Constructor ||
        p->kind == TSK_GetAccessor || p->kind == TSK_SetAccessor) {
      if (bModifierFlags(p) & TMF_Static) return cUnported("this in a static member");
    }
    if (p->kind == TSK_ClassDeclaration || p->kind == TSK_ClassExpression) break;
    if (p->kind == TSK_SourceFile) break;
  }
  TsType *c = cEnclosingClassType(e);
  return c ? c : cUnported("this outside a class");
}

static TsType *cWidenedExpr(TsNode *e) { return cWidenLiteralType(cExprType(e)); }

/* the object type of a member access: the apparent type of a primitive, any for any */
static TsType *cApparentOf(TsType *o) {
  if (o->flags & TF_Any) return o;
  if (o->flags & (TF_String | TF_StringLiteral | TF_Number | TF_NumberLiteral | TF_Boolean | TF_BooleanLiteral | TF_BigInt | TF_ESSymbol | TF_UniqueESSymbol)) {
    TsType *a = cApparentType(o);
    return a ? a : cUnported("apparent type of a primitive");
  }
  return o;
}

/* a property of an object type, or the error type */
static TsType *cMemberType(TsType *o, TsSymbol *p, TsNode *e) {
  (void)o;
  if (!p) return cUnported("member not found");
  (void)e;
  return cTypeOfSymbol(p);
}

/* the type of a member access e.expression.name (checkPropertyAccessExpression) */
static TsType *cPropertyAccessType(TsNode *e) {
  if (e->questionDotToken) return cUnported("optional chain");
  TsType *o = cExprType(e->expression);
  if (o == cErrorType) return o;
  if (o->flags & TF_Any) return o;
  if (!e->name || e->name->kind != TSK_Identifier) return cUnported("private name access");
  o = cApparentOf(o);
  if (o == cErrorType) return o;
  if (!(o->flags & TF_Object)) return cUnported("property access on a non-object");
  TsSymbol *p = cPropertyOf(o, bnEscapeText(e->name->text, e->name->textLen));
  if (!p) return cUnported("property access: no property");
  TsType *declared = cMemberType(o, p, e);
  if (declared == cErrorType) return declared;
  return cAccessReferenceFlow(e, declared, true);
}

/* the unique symbol key of an element access, the name of its property */
static TsSymbol *cElementAccessProperty(TsType *o, TsNode *e) {
  TsNode *k = cSkipParens(e->argumentExpression);
  if (!k) return NULL;
  if (k->kind == TSK_StringLiteral) return cPropertyOf(o, bnEscapeText(k->text, k->textLen));
  if (k->kind == TSK_NumericLiteral) {
    double v = cNumericLiteralValue(k->text, k->textLen);
    char buf[64];
    int n = snprintf(buf, sizeof buf, "%.17g", v);
    if (v == (double)(long long)v) n = snprintf(buf, sizeof buf, "%lld", (long long)v);
    return cPropertyOf(o, bnEscapeText(buf, n));
  }
  return NULL;
}

static TsType *cElementAccessType(TsNode *e) {
  if (e->questionDotToken) return cUnported("optional chain");
  TsType *o = cExprType(e->expression);
  if (o == cErrorType) return o;
  TsNode *k = cSkipParens(e->argumentExpression);
  if (!k) return cUnported("element access without argument");
  if (o->flags & TF_Any) {
    cExprType(e->argumentExpression);
    return o;
  }
  TsType *kt = cExprType(e->argumentExpression);
  if (kt == cErrorType) return kt;
  o = cApparentOf(o);
  if (o == cErrorType) return o;
  if (!(o->flags & TF_Object)) return cUnported("element access on a non-object");
  if (kt->flags & TF_UniqueESSymbol) {
    TsSymbol *p = cPropertyOf(o, (BName){ kt->text, kt->textLen });
    if (!p) return cUnported("element access: no unique symbol member");
    return cMemberType(o, p, e);
  }
  if (k->kind == TSK_StringLiteral || k->kind == TSK_NumericLiteral) {
    TsSymbol *p = cElementAccessProperty(o, e);
    if (!p) return cUnported("element access: no property");
    TsType *declared = cMemberType(o, p, e);
    if (declared == cErrorType) return declared;
    return cAccessReferenceFlow(e, declared, true);
  }
  if (kt->flags & TF_NumberLiteral) return cUnported("element access by a number literal type");
  if (kt == cNumberType) {
    cResolveIndexInfos(o);
    for (int i = 0; i < o->nindex; i++)
      if (o->index[i].keyType == cNumberType) return o->index[i].type;
    return cUnported("element access without a number index");
  }
  return cUnported("element access by a computed key");
}

/* the type of an assignment target, read without the flow (checkIdentifier and checkPropertyAccessExpression for
 * an assignment target have the declared type) */
static TsType *cAssignmentTargetType(TsNode *left) {
  TsNode *l = cSkipParens(left);
  if (l->kind == TSK_Identifier) {
    TsSymbol *s = cResolveValueName(l, bnEscapeText(l->text, l->textLen));
    if (!s) return cUnported("assignment to an unresolved name");
    if (s->flags & SF_Alias) return cUnported("assignment to an import");
    if (!(s->flags & SF_Variable)) return cUnported("assignment to a non-variable");
    return cTypeOfSymbol(s);
  }
  if (l->kind == TSK_PropertyAccessExpression && !l->questionDotToken && l->name && l->name->kind == TSK_Identifier) {
    TsType *o = cExprType(l->expression);
    if (o == cErrorType) return o;
    if (o->flags & TF_Any) return o;
    o = cApparentOf(o);
    if (o == cErrorType) return o;
    if (!(o->flags & TF_Object)) return cUnported("assignment to a member of a non-object");
    TsSymbol *p = cPropertyOf(o, bnEscapeText(l->name->text, l->name->textLen));
    if (!p) return cUnported("assignment to a missing member");
    return cTypeOfSymbol(p);
  }
  return cUnported("assignment target");
}

static bool cIsNumberLike(TsType *t) { return (t->flags & (TF_Number | TF_NumberLiteral | TF_Enum)) != 0; }
static bool cIsStringLike(TsType *t) { return (t->flags & (TF_String | TF_StringLiteral)) != 0; }

/* the typeof operator's result: the union of the eight type names, as literal types */
static TsType *cTypeofUnion(void) {
  static const char *names[] = { "string", "number", "bigint", "boolean", "symbol", "undefined", "object", "function" };
  TsType *v[8];
  for (int i = 0; i < 8; i++) v[i] = cLiteralType(TF_StringLiteral, names[i], (int)strlen(names[i]));
  return cGetUnionType(v, 8);
}

/* the type of a call or new expression of a single non-generic signature (resolveCall without overloads) */
static TsType *cCallSignatureType(TsNode *e, TsType *f, bool isNew) {
  if (f == cAnyType || f == cErrorType) {
    /* untyped call: the arguments are checked, the result is any */
    if (e->arguments) for (int i = 0; i < e->arguments->n; i++) cExprType(e->arguments->v[i]);
    return f == cErrorType ? f : cAnyType;
  }
  if (!(f->flags & TF_Object)) return cUnported("call of a non-object");
  cResolveSignatures(f);
  int ns = isNew ? f->nconstruct : f->ncall;
  CSig *sigs = isNew ? f->construct : f->call;
  if (ns != 1) return cUnported("call with other than one signature");
  CSig *sig = &sigs[0];
  if (sig->decl->typeParameters && sig->decl->typeParameters->n) return cUnported("call of a generic signature");
  if (e->typeArguments) return cUnported("call with type arguments");
  CParams ps;
  if (!cSplitParams(sig->decl, &ps)) return cUnported("too many parameters");
  if (ps.thisParam || ps.rest) return cUnported("call with a this or rest parameter");
  int na = e->arguments ? e->arguments->n : 0;
  TsType **at = ts_arena_alloc(sizeof(TsType *) * (size_t)(na ? na : 1));
  for (int i = 0; i < na; i++) at[i] = cExprType(e->arguments->v[i]);
  if (na < ps.min || na > ps.n) return cUnported("call with a wrong argument count");
  for (int i = 0; i < na; i++) {
    if (at[i] == cErrorType) continue;
    bool ok;
    TsType *pt = cParamTypeAt(&ps, i, &ok, sig->mapper);
    if (!ok || pt == cErrorType) continue;
    cIsRelatedWith(at[i], pt, REL_Assignable);
  }
  TsType *rt = cReturnTypeOf(sig);
  /* checkCallExpression: Symbol() and Symbol.for() give the unique symbol of the declaration they initialize */
  if (!isNew && rt != cErrorType && (rt->flags & (TF_ESSymbol | TF_UniqueESSymbol)) && cIsSymbolCall(e)) {
    TsNode *p = e->parent;
    while (p && p->kind == TSK_ParenthesizedExpression) p = p->parent;
    if (p && p->kind == TSK_VariableDeclaration && cIsConstVariableDecl(p)) return cUniqueSymbolType(p);
    return cESSymbolType;
  }
  return rt;
}

static TsType *cCallType(TsNode *e) {
  if (e->expression->kind == TSK_SuperKeyword) return cUnported("super call");
  if (e->questionDotToken) return cUnported("optional call");
  TsType *f = cExprType(e->expression);
  if (f == cErrorType) {
    if (e->arguments) for (int i = 0; i < e->arguments->n; i++) cExprType(e->arguments->v[i]);
    return f;
  }
  return cCallSignatureType(e, f, false);
}

static TsType *cNewType2(TsNode *e) {
  if (e->typeArguments) return cUnported("new with type arguments");
  TsType *f = cExprType(e->expression);
  if (f == cErrorType) return f;
  if (f == cAnyType) return cCallSignatureType(e, f, true);
  return cCallSignatureType(e, f, true);
}

/* the element type of an array literal: the subtype-reduced union of the widened elements */
static TsType *cArrayLiteralType(TsNode *e) {
  if (!e->elements || e->elements->n == 0) return cUnported("empty array literal (evolving array)");
  int n = e->elements->n;
  TsType **ts = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
  for (int i = 0; i < n; i++) {
    TsNode *el = e->elements->v[i];
    if (el->kind == TSK_SpreadElement) return cUnported("spread in an array literal");
    if (el->kind == TSK_OmittedExpression) { ts[i] = cUndefinedType; continue; }
    TsType *t = cWidenedExpr(el);
    if (t == cErrorType) return t;
    ts[i] = t;
  }
  TsType *elem = n == 1 ? ts[0] : cUnionWithSubtypesRemoved(ts, n);
  if (elem == cErrorType) return elem;
  TsSymbol *arr = bTableGet(cGlobals, bnC("Array"));
  if (!arr) return cUnported("no global Array");
  TsType *target = cDeclaredTypeOfClassOrInterface(arr);
  TsType *ref = cTypeReference(target, &elem, 1);
  if (!ref->literalType) {
    /* cloneTypeReference: the same target and arguments, a type of its own that carries ArrayLiteral */
    TsType *c = cNewType(ref->flags);
    c->symbol = ref->symbol;
    c->objectFlags = ref->objectFlags | OF_ArrayLiteral;
    c->target = ref->target;
    c->args = ref->args;
    c->nargs = ref->nargs;
    ref->literalType = c;
  }
  return ref->literalType;
}

/* the type of an object literal without a contextual type (createObjectLiteralType over the literal's symbol) */
static TsType *cObjectLiteralType(TsNode *e) {
  if (!e->symbol) return cUnported("object literal without symbol");
  if (!e->properties) return cUnported("object literal without properties");
  for (int i = 0; i < e->properties->n; i++) {
    TsNode *p = e->properties->v[i];
    switch (p->kind) {
      case TSK_PropertyAssignment: case TSK_ShorthandPropertyAssignment: case TSK_MethodDeclaration:
      case TSK_GetAccessor: case TSK_SetAccessor:
        break;
      default: return cUnported("spread or other member of an object literal");
    }
  }
  TsType *t = cNewType(TF_Object);
  t->objectFlags = OF_ObjectLiteral;
  t->symbol = cGetMergedSymbol(e->symbol);
  return t;
}

/* the value of a numeric literal or of its negation (checkPrefixUnaryExpression: one fresh literal) */
static TsType *cNumericLiteralExpr(TsNode *l, bool negative) {
  if (cHasEscapeOrNonAscii(l->text, l->textLen) || memchr(l->text, '_', (size_t)l->textLen)) return cUnported("numeric literal text");
  TsType *r;
  if (negative) {
    r = cLiteralType(TF_NumberLiteral, cArenaCat("-", 1, l->text, l->textLen), l->textLen + 1);
    r->litNum = -cNumericLiteralValue(l->text, l->textLen);
  } else {
    r = cLiteralType(TF_NumberLiteral, l->text, l->textLen);
    r->litNum = cNumericLiteralValue(l->text, l->textLen);
  }
  return cFreshLiteral(r);
}

/* the type of an assignment `left = right` (checkBinaryLikeExpression, EqualsToken): the right type */
static TsType *cAssignmentExpr(TsNode *e) {
  TsNode *l = cSkipParens(e->left);
  if (l->kind == TSK_ElementAccessExpression) {
    /* this[K] targets are checked by cCheckAssignmentOperator (support/ts-checker-expr.h) */
    TsType *r = cExprType(e->right);
    (void)r;
    return r;
  }
  TsType *rt = cExprType(e->right);
  TsType *lt = cAssignmentTargetType(e->left);
  if (rt == cErrorType || lt == cErrorType) return rt;
  if (e->right->kind == TSK_ObjectLiteralExpression || e->right->kind == TSK_ArrayLiteralExpression ||
      e->right->kind == TSK_ArrowFunction || e->right->kind == TSK_FunctionExpression) return rt;
  if (!cIsRelatedWith(rt, lt, REL_Assignable)) cDiagAdd(cSourceFileOf(e->left), cNodeStart(e->left), 2322, NULL);
  return rt;
}

static TsType *cBinaryType(TsNode *e) {
  int op = e->operatorToken->kind;
  switch (op) {
    case TSK_EqualsToken: return cAssignmentExpr(e);
    case TSK_CommaToken: cExprType(e->left); return cExprType(e->right);
    case TSK_EqualsEqualsToken: case TSK_ExclamationEqualsToken: case TSK_EqualsEqualsEqualsToken:
    case TSK_ExclamationEqualsEqualsToken: case TSK_LessThanToken: case TSK_GreaterThanToken:
    case TSK_LessThanEqualsToken: case TSK_GreaterThanEqualsToken: case TSK_InstanceOfKeyword: case TSK_InKeyword: {
      TsType *l = cExprType(e->left), *r = cExprType(e->right);
      if (l == cErrorType || r == cErrorType) return cUnported("comparison of an unported operand");
      return cBooleanType;
    }
    case TSK_PlusToken: {
      TsType *l = cExprType(e->left), *r = cExprType(e->right);
      if (l == cErrorType || r == cErrorType) return cUnported("sum of an unported operand");
      if (cIsNumberLike(l) && cIsNumberLike(r)) return cNumberType;
      if (cIsStringLike(l) || cIsStringLike(r)) return cStringType;
      return cUnported("sum of other types");
    }
    case TSK_MinusToken: case TSK_AsteriskToken: case TSK_SlashToken: case TSK_PercentToken:
    case TSK_LessThanLessThanToken: case TSK_GreaterThanGreaterThanToken: case TSK_GreaterThanGreaterThanGreaterThanToken:
    case TSK_AmpersandToken: case TSK_BarToken: case TSK_CaretToken: {
      TsType *l = cExprType(e->left), *r = cExprType(e->right);
      if (l == cErrorType || r == cErrorType) return cUnported("arithmetic on an unported operand");
      if ((l->flags | r->flags) & TF_BigInt) return cUnported("arithmetic on bigint");
      return cNumberType;
    }
    default: break;
  }
  return cUnported("binary operator");
}

static TsType *cExprWorker(TsNode *e) {
  switch (e->kind) {
    case TSK_ParenthesizedExpression: return cExprType(e->expression);
    case TSK_Identifier: return cIdentifierType(e);
    case TSK_ThisKeyword: return cThisType(e);
    case TSK_NullKeyword: return cNullType;
    case TSK_TrueKeyword: return cTrueType;
    case TSK_FalseKeyword: return cFalseType;
    case TSK_StringLiteral: return cFreshLiteral(cLiteralType(TF_StringLiteral, e->text, e->textLen));
    case TSK_NoSubstitutionTemplateLiteral:
      if (cHasEscapeOrNonAscii(e->text, e->textLen)) return cUnported("template text with an escape");
      return cFreshLiteral(cLiteralType(TF_StringLiteral, e->text, e->textLen));
    case TSK_NumericLiteral: return cNumericLiteralExpr(e, false);
    case TSK_PrefixUnaryExpression: {
      if (e->extra == TSK_MinusToken && cSkipParens(e->operand)->kind == TSK_NumericLiteral) return cNumericLiteralExpr(cSkipParens(e->operand), true);
      if (e->extra == TSK_ExclamationToken) { cExprType(e->operand); return cBooleanType; }
      TsType *o = cExprType(e->operand);
      if (o == cErrorType) return o;
      if (o->flags & TF_BigInt) return cUnported("unary operator on bigint");
      return cNumberType;
    }
    case TSK_PostfixUnaryExpression: {
      TsType *o = cExprType(e->operand);
      if (o == cErrorType) return o;
      if (o->flags & TF_BigInt) return cUnported("unary operator on bigint");
      return cNumberType;
    }
    case TSK_TypeOfExpression: {
      TsType *o = cExprType(e->expression);
      if (o == cErrorType) return o;
      return cTypeofUnion();
    }
    case TSK_VoidExpression: cExprType(e->expression); return cUndefinedType;
    case TSK_DeleteExpression: cExprType(e->expression); return cBooleanType;
    case TSK_AsExpression: {
      cExprType(e->expression);
      return cTypeFromNode(e->type);
    }
    case TSK_NonNullExpression: {
      TsType *o = cExprType(e->expression);
      if (o == cErrorType) return o;
      return cCheckNonNullable(o);
    }
    case TSK_BinaryExpression: return cBinaryType(e);
    case TSK_ConditionalExpression: {
      cExprType(e->condition);
      TsType *a = cExprType(e->whenTrue), *b = cExprType(e->whenFalse);
      if (a == cErrorType || b == cErrorType) return cUnported("conditional of an unported branch");
      if (a == b) return a;
      TsType *v[2] = { a, b };
      return cUnionWithSubtypesRemoved(v, 2);
    }
    case TSK_PropertyAccessExpression: return cPropertyAccessType(e);
    case TSK_ElementAccessExpression: return cElementAccessType(e);
    case TSK_CallExpression: return cCallType(e);
    case TSK_NewExpression: return cNewType2(e);
    case TSK_ArrayLiteralExpression:
      if (cContextualType(e) != NULL) return cUnported("array literal with a contextual type");
      return cArrayLiteralType(e);
    case TSK_ObjectLiteralExpression:
      if (cContextualType(e) != NULL) return cUnported("object literal with a contextual type");
      return cObjectLiteralType(e);
    case TSK_FunctionExpression: case TSK_ArrowFunction:
      if (cContextualType(e) != NULL) return cUnported("function expression with a contextual type");
      if (!e->symbol) return cUnported("function expression without symbol");
      return cTypeOfSymbol(e->symbol);
    default: break;
  }
  return cUnported("expression type");
}


/* ---- statements and declarations (checkSourceElement) ---- */
static void cCheckSourceElement(TsNode *n);

static void cCheckStatementList(TsList *l) {
  if (!l) return;
  for (int i = 0; i < l->n; i++) cCheckSourceElement(l->v[i]);
}

/* the expression of an initializer is related to the declared type of the declaration (checkVariableLikeDeclaration) */
static void cCheckInitializer(TsNode *d, TsType *target) {
  TsType *src = cExprType(d->initializer);
  if (src == cErrorType || target == cErrorType) return;
  TsNode *init = cSkipParens(d->initializer);
  bool elaborates = init->kind == TSK_ObjectLiteralExpression || init->kind == TSK_ArrayLiteralExpression ||
                    init->kind == TSK_ArrowFunction;
  if (cIsRelatedWith(src, target, REL_Assignable)) return;
  if (init->kind == TSK_ObjectLiteralExpression) {
    cElaborateObjectLiteral(init, target);
    return;
  }
  if (elaborates) return;   /* not elaborated here: no report (no false diagnostics) */
  if (d->kind == TSK_VariableDeclaration || d->kind == TSK_PropertyDeclaration || d->kind == TSK_Parameter)
    cDiagAdd(cSourceFileOf(d->name ? d->name : d), cNodeStart(d->name ? d->name : d), 2322, NULL);
}

/* checkVariableLikeDeclaration for a declaration with a name that is an identifier */
static void cCheckVariableLike2(TsNode *d) {
  if (!d->name) return;
  if (d->initializer) {
    if (d->name->kind == TSK_Identifier) {
      TsSymbol *s = d->symbol ? cGetMergedSymbol(d->symbol) : NULL;
      TsType *target = s ? cTypeOfSymbol(s) : cErrorType;
      if (d->kind == TSK_Parameter && !(d->questionToken || d->initializer)) target = cErrorType;
      cCheckInitializer(d, target);
    } else {
      cExprType(d->initializer);   /* binding patterns: the destructuring checks are not ported */
    }
  }
}

/* checkReturnStatement: the expression is related to the annotated return type of its function */
static void cCheckReturn(TsNode *n) {
  TsNode *fn = n->parent;
  while (fn && !cIsFunctionLikeKind(fn->kind)) fn = fn->parent;
  if (!n->expression) return;
  TsType *src = cExprType(n->expression);
  if (!fn || !fn->type || fn->asteriskToken || (bModifierFlags(fn) & TMF_Async)) return;
  if (fn->kind == TSK_SetAccessor || fn->kind == TSK_Constructor) return;
  TsType *target = cTypeFromNode(fn->type);
  if (src == cErrorType || target == cErrorType) return;
  TsNode *init = cSkipParens(n->expression);
  if (cIsRelatedWith(src, target, REL_Assignable)) return;
  if (init->kind == TSK_ObjectLiteralExpression) { cElaborateObjectLiteral(init, target); return; }
  if (init->kind == TSK_ArrayLiteralExpression || init->kind == TSK_ArrowFunction) return;
  cDiagAdd(cSourceFileOf(n), cNodeStart(n), 2322, NULL);
}

/* a function-like node: its parameters and its body (checkFunctionOrMethodDeclaration, checkFunctionExpressionOrObjectLiteralMethodDeferred) */
static void cCheckFunctionLikeBody(TsNode *n) {
  if (n->parameters) for (int i = 0; i < n->parameters->n; i++) cCheckSourceElement(n->parameters->v[i]);
  if (!n->body) return;
  if (n->body->kind == TSK_Block) { cCheckSourceElement(n->body); return; }
  TsType *src = cExprType(n->body);
  if (!n->type || n->asteriskToken || (bModifierFlags(n) & TMF_Async) || src == cErrorType) return;
  TsType *target = cTypeFromNode(n->type);
  if (target == cErrorType || cIsRelatedWith(src, target, REL_Assignable)) return;
  TsNode *b = cSkipParens(n->body);
  if (b->kind == TSK_ObjectLiteralExpression) { cElaborateObjectLiteral(b, target); return; }
  if (b->kind == TSK_ArrayLiteralExpression) return;
  cDiagAdd(cSourceFileOf(n->body), cNodeStart(n->body), 2322, NULL);
}

static void cCheckSourceElement(TsNode *n) {
  if (!n || n->checkState) return;
  n->checkState = 1;
  switch (n->kind) {
    case TSK_SourceFile: cCheckStatementList(n->statements); break;
    case TSK_VariableStatement: cCheckSourceElement(n->declarationList); break;
    case TSK_VariableDeclarationList:
      if (n->declarations) for (int i = 0; i < n->declarations->n; i++) cCheckSourceElement(n->declarations->v[i]);
      break;
    case TSK_VariableDeclaration: case TSK_Parameter: case TSK_PropertyDeclaration:
      cCheckVariableLike2(n);
      break;
    case TSK_ExpressionStatement: cExprType(n->expression); break;
    case TSK_IfStatement:
      cExprType(n->expression);
      cCheckSourceElement(n->thenStatement);
      cCheckSourceElement(n->elseStatement);
      break;
    case TSK_DoStatement: case TSK_WhileStatement:
      cExprType(n->expression);
      cCheckSourceElement(n->statement);
      break;
    case TSK_ForStatement:
      if (n->initializer) {
        if (n->initializer->kind == TSK_VariableDeclarationList) cCheckSourceElement(n->initializer);
        else cExprType(n->initializer);
      }
      if (n->condition) cExprType(n->condition);
      if (n->incrementor) cExprType(n->incrementor);
      cCheckSourceElement(n->statement);
      break;
    case TSK_ForInStatement: case TSK_ForOfStatement:
      if (n->initializer) {
        if (n->initializer->kind == TSK_VariableDeclarationList) cCheckSourceElement(n->initializer);
        else cExprType(n->initializer);
      }
      cExprType(n->expression);
      cCheckSourceElement(n->statement);
      break;
    case TSK_ReturnStatement: cCheckReturn(n); break;
    case TSK_ThrowStatement: cExprType(n->expression); break;
    case TSK_SwitchStatement:
      cExprType(n->expression);
      cCheckSourceElement(n->caseBlock);
      break;
    case TSK_CaseBlock:
      if (n->clauses) for (int i = 0; i < n->clauses->n; i++) cCheckSourceElement(n->clauses->v[i]);
      break;
    case TSK_CaseClause:
      cExprType(n->expression);
      cCheckStatementList(n->statements);
      break;
    case TSK_DefaultClause: cCheckStatementList(n->statements); break;
    case TSK_TryStatement:
      cCheckSourceElement(n->block);
      cCheckSourceElement(n->catchClause);
      cCheckSourceElement(n->finallyBlock);
      break;
    case TSK_CatchClause: cCheckSourceElement(n->block); break;
    case TSK_Block: case TSK_ModuleBlock: cCheckStatementList(n->statements); break;
    case TSK_LabeledStatement: cCheckSourceElement(n->statement); break;
    case TSK_EmptyStatement: case TSK_BreakStatement: case TSK_ContinueStatement: case TSK_DebuggerStatement:
    case TSK_InterfaceDeclaration: case TSK_TypeAliasDeclaration: case TSK_EnumDeclaration: case TSK_ImportDeclaration:
    case TSK_ImportEqualsDeclaration: case TSK_ExportDeclaration: case TSK_NamespaceExportDeclaration:
    case TSK_TypeParameter: case TSK_IndexSignature: case TSK_SemicolonClassElement: case TSK_EnumMember:
      break;
    case TSK_FunctionDeclaration: case TSK_MethodDeclaration: case TSK_Constructor: case TSK_GetAccessor:
    case TSK_SetAccessor: case TSK_FunctionExpression: case TSK_ArrowFunction:
      cCheckFunctionLikeBody(n);
      break;
    case TSK_ClassDeclaration: case TSK_ClassExpression:
      if (n->members) for (int i = 0; i < n->members->n; i++) cCheckSourceElement(n->members->v[i]);
      break;
    case TSK_ClassStaticBlockDeclaration: cCheckSourceElement(n->body); break;
    case TSK_ModuleDeclaration: cCheckSourceElement(n->body); break;
    case TSK_ExportAssignment: cExprType(n->expression); break;
    default:
      cUnported("statement");
      break;
  }
  n->checkState = 2;
}

/* checkSourceFile of the bodies: the statements, then every function-like node that no statement reached (the
 * functions in expressions, which are checked deferred in node) */
static void cCheckFunctionPending(TsNode *n) {
  if (cIsFunctionLikeKind(n->kind) && n->checkState == 0) cCheckSourceElement(n);
}

static void cCheckBodies(TsNode *sf) {
  cCheckSourceElement(sf);
  cWalkAll(sf, cCheckFunctionPending);
}

#endif
