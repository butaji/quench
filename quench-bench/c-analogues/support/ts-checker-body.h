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
static TsType *cAddUndefined(TsType *t) { return cAddOptionality(t); }

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
  bool jump;             /* a const reference continues at the creation flow of each function value between it and its declaration */
  TsNode *flowContainer; /* the container the walk ends at (checkIdentifier's flowContainer) */
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

/* isMatchingReference: the target is the reference itself */
static bool cAssignMatches(TsNode *target, CFlowRef *r) {
  TsNode *t = cSkipParens(target);
  if (cSameChain(t, r->node)) return true;
  return false;
}

/* containsMatchingReference: the target is an access chain that contains the reference (x.y = ... for x) */
static bool cAssignContains(TsNode *target, CFlowRef *r) {
  TsNode *t = cSkipParens(target);
  while (t && (t->kind == TSK_PropertyAccessExpression || t->kind == TSK_ElementAccessExpression)) {
    t = cSkipParens(t->expression);
    if (cSameChain(t, r->node)) return true;
  }
  return false;
}

static bool cOuterNarrowingSafe(TsType *d) {
  if (d->flags & TF_Union) return false;
  return (d->flags & (TF_Object | TF_String | TF_Number | TF_BigInt | TF_ESSymbol | TF_UniqueESSymbol | TF_NonPrimitive |
                      TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral | TF_Undefined | TF_Null | TF_Void)) != 0;
}

/* the type of a reference at the start of its container (getTypeAtFlowNode, FlowStart) */
static TsType *cFlowStartType(CFlowRef *r) {
  if (r->jump) return r->startInit ? r->declared : cUnported("reference used before its assignment");
  if (r->outer) return cOuterNarrowingSafe(r->declared) ? r->declared : cUnported("narrowing of an outer reference");
  return r->startInit ? r->declared : cUnported("reference used before its assignment");
}

static bool cMatchesRefTree(TsNode *n, CFlowRef *r) { return cSameChain(n, r->node); }

/* the object of a reference chain (x for x.y): the reference is the object of a member access */
static bool cIsMemberOfRef(TsNode *n, CFlowRef *r) {
  n = cSkipParens(n);
  return n && (n->kind == TSK_PropertyAccessExpression || n->kind == TSK_ElementAccessExpression) && cMatchesRefTree(n->expression, r);
}

/* TypeFacts (TypeScript 5.9.2, values of the enum TypeFacts; getTypeFactsWorker for the ported kinds) */
enum {
  TFC_TypeofEQString = 1, TFC_TypeofEQNumber = 2, TFC_TypeofEQBigInt = 4, TFC_TypeofEQBoolean = 8, TFC_TypeofEQSymbol = 16,
  TFC_TypeofEQObject = 32, TFC_TypeofEQFunction = 64, TFC_TypeofEQHostObject = 128, TFC_TypeofNEString = 256,
  TFC_TypeofNENumber = 512, TFC_TypeofNEBigInt = 1024, TFC_TypeofNEBoolean = 2048, TFC_TypeofNESymbol = 4096,
  TFC_TypeofNEObject = 8192, TFC_TypeofNEFunction = 16384, TFC_TypeofNEHostObject = 32768, TFC_EQUndefined = 65536,
  TFC_EQNull = 131072, TFC_EQUndefinedOrNull = 262144, TFC_NEUndefined = 524288, TFC_NENull = 1048576,
  TFC_NEUndefinedOrNull = 2097152, TFC_Truthy = 4194304, TFC_Falsy = 8388608
};
enum {
  TFC_StringStrict = 16317953, TFC_EmptyStringStrict = 12123649, TFC_NonEmptyStringStrict = 7929345,
  TFC_NumberStrict = 16317698, TFC_ZeroNumberStrict = 12123394, TFC_NonZeroNumberStrict = 7929090,
  TFC_BigIntStrict = 16317188, TFC_BooleanStrict = 16316168, TFC_FalseStrict = 12121864, TFC_TrueStrict = 7927560,
  TFC_SymbolStrict = 7925520, TFC_ObjectStrict = 7888800, TFC_FunctionStrict = 7880640, TFC_EmptyObjectStrict = 83427327,
  TFC_VoidFacts = 9830144, TFC_UndefinedFacts = 26607360, TFC_NullFacts = 42917664, TFC_UnknownFacts = 83886079
};

/* getTypeFactsWorker; *ok is false when a member is not ported */
static uint32_t cTypeFacts(TsType *t, bool *ok) {
  uint32_t f = t->flags;
  if (f & TF_Boolean) return TFC_BooleanStrict;              /* the boolean union is tested before its members */
  if (f & TF_Union) {
    uint32_t r = 0;
    for (int i = 0; i < t->ntypes; i++) r |= cTypeFacts(t->types[i], ok);
    return r;
  }
  if (f & TF_String) return TFC_StringStrict;
  if (f & TF_StringLiteral) return t->textLen == 0 ? TFC_EmptyStringStrict : TFC_NonEmptyStringStrict;
  if (f & TF_EnumLiteral) { *ok = false; return 0; }
  if (f & (TF_Number | TF_Enum)) return TFC_NumberStrict;
  if (f & TF_NumberLiteral) return t->litNum == 0 ? TFC_ZeroNumberStrict : TFC_NonZeroNumberStrict;
  if (f & TF_BigInt) return TFC_BigIntStrict;
  if (f & TF_BooleanLiteral) return t == cTrueType ? TFC_TrueStrict : TFC_FalseStrict;
  if (f & TF_Object) {
    cResolveSignatures(t);
    if (t->ncall || t->nconstruct) return TFC_FunctionStrict;
    TsSymbol **props;
    int np = cPropertiesOf(t, &props);
    cResolveIndexInfos(t);
    if ((t->objectFlags & OF_Anonymous) && np == 0 && t->nindex == 0) return TFC_EmptyObjectStrict;
    return TFC_ObjectStrict;
  }
  if (f & (TF_ESSymbol | TF_UniqueESSymbol)) return TFC_SymbolStrict;
  if (f & TF_NonPrimitive) return TFC_ObjectStrict;
  if (f & TF_Void) return TFC_VoidFacts;
  if (f & TF_Undefined) return TFC_UndefinedFacts;
  if (f & TF_Null) return TFC_NullFacts;
  if (f & TF_Never) return 0;
  if (f & (TF_Any | TF_Unknown)) return TFC_UnknownFacts;
  *ok = false;
  return 0;
}

/* the members of a type (a union's members, or the type itself) */
static int cMemberCount(TsType *t) { return (t->flags & TF_Union) ? t->ntypes : 1; }
static TsType *cMemberAt(TsType *t, int i) { return (t->flags & TF_Union) ? t->types[i] : t; }

/* getTypeWithFacts: the members that have one of the facts. *ok is false when a member is not ported. */
static TsType *cTypeWithFacts(TsType *t, uint32_t facts) {
  if (t == cErrorType) return t;
  int n = cMemberCount(t);
  TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  int k = 0;
  bool changed = false;
  for (int i = 0; i < n; i++) {
    TsType *m = cMemberAt(t, i);
    bool ok = true;
    uint32_t mf = cTypeFacts(m, &ok);
    if (!ok) return cUnported("facts of a type that is not ported");
    if (mf & facts) kept[k++] = m; else changed = true;
  }
  if (!changed) return t;
  if (k == 0) return cNeverType;
  return k == 1 ? kept[0] : cGetUnionType(kept, k);
}

/* narrowTypeByTruthiness: the truthy or falsy part of a type (getTypeWithFacts with Truthy or Falsy) */
static TsType *cTruthiness(TsType *t, bool truthy) {
  if (t == cErrorType || (t->flags & TF_Any)) return t;
  return cTypeWithFacts(t, truthy ? TFC_Truthy : TFC_Falsy);
}

/* narrowTypeByEquality against undefined or null (getAdjustedTypeWithFacts with the facts of the comparison) */
static TsType *cNullishEquality(TsType *t, TsType *value, bool doubleEquals, bool assumeTrue) {
  if (t == cErrorType || (t->flags & TF_Any)) return t;
  uint32_t facts;
  if (doubleEquals) facts = assumeTrue ? TFC_EQUndefinedOrNull : TFC_NEUndefinedOrNull;
  else if (value == cNullType) facts = assumeTrue ? TFC_EQNull : TFC_NENull;
  else facts = assumeTrue ? TFC_EQUndefined : TFC_NEUndefined;
  return cTypeWithFacts(t, facts);
}

/* the facts of a typeof name, for the true branch (typeofEQFacts) and the false branch (typeofNEFacts) */
static uint32_t cTypeofEqFacts(const char *n, int len, bool *ok) {
  *ok = true;
  if (len == 6 && memcmp(n, "string", 6) == 0) return TFC_TypeofEQString;
  if (len == 6 && memcmp(n, "number", 6) == 0) return TFC_TypeofEQNumber;
  if (len == 6 && memcmp(n, "bigint", 6) == 0) return TFC_TypeofEQBigInt;
  if (len == 7 && memcmp(n, "boolean", 7) == 0) return TFC_TypeofEQBoolean;
  if (len == 6 && memcmp(n, "symbol", 6) == 0) return TFC_TypeofEQSymbol;
  if (len == 9 && memcmp(n, "undefined", 9) == 0) return TFC_EQUndefined;
  if (len == 6 && memcmp(n, "object", 6) == 0) return TFC_TypeofEQObject;
  if (len == 8 && memcmp(n, "function", 8) == 0) return TFC_TypeofEQFunction;
  *ok = false;
  return TFC_TypeofEQHostObject;
}

static uint32_t cTypeofNeFacts(const char *n, int len) {
  if (len == 6 && memcmp(n, "string", 6) == 0) return TFC_TypeofNEString;
  if (len == 6 && memcmp(n, "number", 6) == 0) return TFC_TypeofNENumber;
  if (len == 6 && memcmp(n, "bigint", 6) == 0) return TFC_TypeofNEBigInt;
  if (len == 7 && memcmp(n, "boolean", 7) == 0) return TFC_TypeofNEBoolean;
  if (len == 6 && memcmp(n, "symbol", 6) == 0) return TFC_TypeofNESymbol;
  if (len == 9 && memcmp(n, "undefined", 9) == 0) return TFC_NEUndefined;
  if (len == 6 && memcmp(n, "object", 6) == 0) return TFC_TypeofNEObject;
  if (len == 8 && memcmp(n, "function", 8) == 0) return TFC_TypeofNEFunction;
  return TFC_TypeofNEHostObject;
}

/* narrowTypeByTypeFacts: a member that is a strict subtype of the implied type is kept with the facts, a member that
 * the implied type is a subtype of is replaced by the implied type, and the others are dropped. A member that could
 * still overlap is an intersection, which is not ported. */
static TsType *cNarrowByTypeFacts(TsType *t, TsType *implied, uint32_t facts) {
  if (t == cErrorType) return t;
  int n = cMemberCount(t);
  TsType **res = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  int k = 0;
  for (int i = 0; i < n; i++) {
    TsType *m = cMemberAt(t, i);
    if (m->flags & (TF_Any | TF_Unknown)) return cUnported("typeof narrowing of any or unknown");
    bool ok = true;
    uint32_t mf = cTypeFacts(m, &ok);
    if (!ok) return cUnported("typeof narrowing of a type that is not ported");
    if (cIsRelatedWith(m, implied, REL_StrictSubtype)) {
      if (mf & facts) res[k++] = m;
    } else if (cIsRelatedWith(implied, m, REL_Subtype)) {
      res[k++] = implied;
    } else if (mf & facts) {
      return cUnported("typeof narrowing into an intersection");
    }
  }
  if (k == 0) return cNeverType;
  return k == 1 ? res[0] : cGetUnionType(res, k);
}

/* narrowTypeByTypeName: the implied type of a typeof name */
static TsType *cNarrowByTypeName(TsType *t, const char *n, int len) {
  if (t == cErrorType) return t;
  bool isAny = (t->flags & TF_Any) != 0;
  if (len == 6 && memcmp(n, "string", 6) == 0) return cNarrowByTypeFacts(t, cStringType, TFC_TypeofEQString);
  if (len == 6 && memcmp(n, "number", 6) == 0) return cNarrowByTypeFacts(t, cNumberType, TFC_TypeofEQNumber);
  if (len == 6 && memcmp(n, "bigint", 6) == 0) return cNarrowByTypeFacts(t, cBigIntType, TFC_TypeofEQBigInt);
  if (len == 7 && memcmp(n, "boolean", 7) == 0) return cNarrowByTypeFacts(t, cBooleanType, TFC_TypeofEQBoolean);
  if (len == 6 && memcmp(n, "symbol", 6) == 0) return cNarrowByTypeFacts(t, cESSymbolType, TFC_TypeofEQSymbol);
  if (len == 6 && memcmp(n, "object", 6) == 0) {
    if (isAny) return t;
    TsType *a = cNarrowByTypeFacts(t, cNonPrimitiveType, TFC_TypeofEQObject);
    TsType *b = cNarrowByTypeFacts(t, cNullType, TFC_EQNull);
    if (a == cErrorType || b == cErrorType) return cUnported("typeof object narrowing");
    TsType *v[2] = { a, b };
    return cGetUnionType(v, 2);
  }
  if (len == 8 && memcmp(n, "function", 8) == 0) {
    if (isAny) return t;
    /* a member with signatures is a function: its narrowing to the global Function type is not ported */
    for (int i = 0; i < cMemberCount(t); i++) {
      TsType *m = cMemberAt(t, i);
      if (m->flags & TF_Object) { cResolveSignatures(m); if (m->ncall || m->nconstruct) return cUnported("typeof function narrowing of a function type"); }
    }
    TsSymbol *fs = bTableGet(cGlobals, bnC("Function"));
    if (!fs) return cUnported("no global Function");
    return cNarrowByTypeFacts(t, cDeclaredTypeOfClassOrInterface(fs), TFC_TypeofEQFunction);
  }
  if (len == 9 && memcmp(n, "undefined", 9) == 0) return cNarrowByTypeFacts(t, cUndefinedType, TFC_EQUndefined);
  return cUnported("typeof of an unknown name");
}

/* narrowTypeByTypeof: `typeof ref op "name"` (the name is a string literal) */
static TsType *cNarrowByTypeof(TsNode *typeofExpr, TsNode *lit, bool truthy, TsType *t, CFlowRef *r, bool isEq) {
  (void)typeofExpr;
  (void)r;
  bool assume = isEq ? truthy : !truthy;   /* the comparison holds */
  if (assume) return cNarrowByTypeName(t, lit->text, lit->textLen);
  if (t == cErrorType || (t->flags & TF_Any)) return t;
  return cTypeWithFacts(t, cTypeofNeFacts(lit->text, lit->textLen));
}

/* the callee of a call has a type predicate or an assertion signature (narrowing by a call); *known is false when the
 * callee type is not ported */
static bool cCalleeHasPredicate(TsNode *call, bool *known) {
  *known = false;
  TsType *f = cExprType(call->expression);
  if (f == cErrorType || !(f->flags & TF_Object)) return false;
  cResolveSignatures(f);
  *known = true;
  for (int i = 0; i < f->ncall; i++) {
    TsNode *d = f->call[i].decl;
    if (d && d->type && d->type->kind == TSK_TypePredicate) return true;
  }
  return false;
}

/* the flow type of a reference narrowed by a condition (narrowType): the reference is matched directly, through `!`,
 * by equality with undefined or null, by a typeof comparison, and by calls whose arguments include it (without a type
 * predicate). Discriminants, instanceof, in, literal equality and type predicates are not ported. A condition that does
 * not mention the reference does not narrow it. */
static TsType *cNarrowByCondition(TsNode *c, bool truthy, TsType *t, CFlowRef *r) {
  c = cSkipParens(c);
  if (t == cErrorType || !cMentionsText(c, r->text, r->len)) return t;
  if (c->kind == TSK_PrefixUnaryExpression && c->extra == TSK_ExclamationToken) return cNarrowByCondition(c->operand, !truthy, t, r);
  if (cMatchesRefTree(c, r)) return cTruthiness(t, truthy);
  if (c->kind == TSK_BinaryExpression) {
    int op = c->operatorToken->kind;
    bool isEq = op == TSK_EqualsEqualsEqualsToken || op == TSK_EqualsEqualsToken;
    bool isNe = op == TSK_ExclamationEqualsEqualsToken || op == TSK_ExclamationEqualsToken;
    if (isEq || isNe) {
      TsNode *l = cSkipParens(c->left), *rt = cSkipParens(c->right);
      bool doubleEq = op == TSK_EqualsEqualsToken || op == TSK_ExclamationEqualsToken;
      /* typeof ref === "name" (either order) */
      TsNode *tof = NULL, *lit = NULL;
      if (l->kind == TSK_TypeOfExpression && rt->kind == TSK_StringLiteral) { tof = l; lit = rt; }
      else if (rt->kind == TSK_TypeOfExpression && l->kind == TSK_StringLiteral) { tof = rt; lit = l; }
      if (tof) {
        TsNode *operand = cSkipParens(tof->expression);
        if (!cMatchesRefTree(operand, r)) return cIsMemberOfRef(operand, r) && (t->flags & TF_Union) ? cUnported("discriminant by typeof") : t;
        bool ok;
        cTypeofEqFacts(lit->text, lit->textLen, &ok);
        if (!ok) return cUnported("typeof comparison with another name");
        return cNarrowByTypeof(tof, lit, truthy, t, r, isEq);
      }
      bool lm = cMatchesRefTree(l, r), rm = cMatchesRefTree(rt, r);
      if (lm || rm) {
        TsNode *other = lm ? rt : l;
        TsType *ov = cExprType(other);
        if (ov == cErrorType) return cUnported("equality with an unported value");
        if (ov->flags & (TF_Undefined | TF_Null)) {
          bool holds = isEq == truthy;   /* the comparison holds on this branch */
          return cNullishEquality(t, ov, doubleEq, holds);
        }
        /* narrowTypeByEquality: the true branch filters by comparability (not ported); the false branch of a unit value
         * filters by comparability too, and any other value leaves the type as it is */
        if (isEq == truthy) return cUnported("equality narrowing with a value");
        if (ov->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral | TF_BooleanLiteral | TF_UniqueESSymbol | TF_EnumLiteral))
          return cUnported("equality narrowing with a unit value");
        return t;
      }
      /* a discriminant: a property of the reference compared (narrowTypeByDiscriminant for a union) */
      if (cIsMemberOfRef(l, r) || cIsMemberOfRef(rt, r)) return (t->flags & TF_Union) ? cUnported("discriminant narrowing") : t;
    }
  }
  if (c->kind == TSK_CallExpression && c->arguments) {
    bool argMentions = false;
    for (int i = 0; i < c->arguments->n; i++) if (cMentionsText(c->arguments->v[i], r->text, r->len)) argMentions = true;
    if (!argMentions) return t;
    bool known;
    bool pred = cCalleeHasPredicate(c, &known);
    if (known && !pred) return t;
    return cUnported("narrowing by a call");
  }
  if (c->kind == TSK_PropertyAccessExpression || c->kind == TSK_ElementAccessExpression) {
    if (cIsMemberOfRef(c, r)) return (t->flags & TF_Union) ? cUnported("discriminant truthiness") : t;
  }
  return cUnported("condition that mentions the reference");
}

/* getAssignmentReducedType: the members of the declared union that the assigned type may be assigned to */
static TsType *cAssignmentReduced(TsType *declared, TsType *assigned) {
  if (declared == assigned) return declared;
  if (assigned == cNeverType) return assigned;
  if (!(declared->flags & TF_Union)) return declared;
  bool aUnion = (assigned->flags & TF_Union) != 0;
  TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)declared->ntypes);
  int k = 0;
  for (int i = 0; i < declared->ntypes; i++) {
    TsType *m = declared->types[i];
    bool maybe = false;
    if (aUnion) {
      for (int j = 0; j < assigned->ntypes && !maybe; j++) maybe = cIsRelatedWith(assigned->types[j], m, REL_Assignable);
    } else maybe = cIsRelatedWith(assigned, m, REL_Assignable);
    if (maybe) kept[k++] = m;
  }
  if (k == 0) return cNeverType;
  TsType *reduced = k == 1 ? kept[0] : cGetUnionType(kept, k);
  if (reduced == cErrorType) return reduced;
  return cIsRelatedWith(assigned, reduced, REL_Assignable) ? reduced : declared;
}

/* the type assigned by an assignment flow node (getAssignedType): the initializer, the right side, or number for ++ and -- */
static TsType *cAssignedType(TsNode *n) {
  switch (n->kind) {
    case TSK_VariableDeclaration:
      if (!n->initializer || (n->name && n->name->kind != TSK_Identifier)) return cUnported("assignment of a pattern");
      return cExprType(n->initializer);
    case TSK_BinaryExpression:
      if (n->operatorToken->kind != TSK_EqualsToken) return cUnported("compound assignment");
      return cExprType(n->right);
    case TSK_PrefixUnaryExpression: case TSK_PostfixUnaryExpression: return cNumberType;
    default: return cUnported("assignment of another kind");
  }
}

/* the flow type of a reference at a flow node (getTypeAtFlowNode) */
static TsType *cFlowWalkType(TsFlow *f, CFlowRef *r);

static TsType *cFlowWalkNode(TsFlow *f, CFlowRef *r) {
  uint32_t fl = f->flags;
  TsType *declared = r->declared;
  if (fl & FF_Unreachable) return cNeverType;
  if (fl & FF_Start) {
    /* FlowStart: a function value created in another container continues at its creation flow */
    TsNode *c = f->node;
    if (r->jump && c && c != r->flowContainer) {
      if (!c->flowNode) return cUnported("function value without a flow node");
      return cFlowWalkType(c->flowNode, r);
    }
    return cFlowStartType(r);
  }
  if (fl & FF_ReduceLabel) {
    if (cRedOverrideN == 32) return cUnported("reduce label depth");
    cRedOverrides[cRedOverrideN].target = f->target;
    cRedOverrides[cRedOverrideN].ants = f->redAnts;
    cRedOverrides[cRedOverrideN].n = f->nRedAnts;
    cRedOverrideN++;
    TsType *t = cFlowWalkType(f->ante, r);
    cRedOverrideN--;
    return t;
  }
  if (fl & (FF_BranchLabel | FF_LoopLabel)) {
    TsFlow **ants = f->labelAnts;
    int n = f->nLabelAnts;
    for (int i = cRedOverrideN - 1; i >= 0; i--)
      if (cRedOverrides[i].target == f) { ants = cRedOverrides[i].ants; n = cRedOverrides[i].n; break; }
    if (n == 0) return cNeverType;
    TsType **ts = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
    int k = 0;
    bool same = true, subtypeReduction = false;
    for (int i = 0; i < n; i++) {
      /* a loop label's partial type (the union so far) is what a revisit from its back edge sees (getTypeAtFlowLoopLabel) */
      TsType *t = cFlowWalkType(ants[i], r);
      if (t == cErrorType) return t;
      if (t == cNeverType) continue;
      if (k > 0 && t != ts[0]) same = false;
      ts[k++] = t;
      if (t != declared && cIsRelatedWith(t, declared, REL_Subtype)) subtypeReduction = true;
      if (fl & FF_LoopLabel) f->walkType = k ? (same ? ts[0] : (subtypeReduction ? cUnionWithSubtypesRemoved(ts, k) : cGetUnionType(ts, k))) : cNeverType;
    }
    TsType *res;
    if (k == 0) res = cNeverType;
    else if (same) res = ts[0];
    else if (subtypeReduction) res = cUnionWithSubtypesRemoved(ts, k);
    else res = cGetUnionType(ts, k);
    if (res == cErrorType) return res;
    if ((fl & FF_LoopLabel) && f->walkRevisited && (f->walkMismatch || f->walkSeen != res)) return cUnported("loop flow without a fixed point");
    return res;
  }
  if (fl & FF_Assignment) {
    TsNode *tg = cAssignTarget(f->node);
    if (tg && cAssignMatches(tg, r)) {
      if (r->nonUnion) return declared;
      TsType *assigned = cAssignedType(f->node);
      if (assigned == cErrorType) return assigned;
      return cAssignmentReduced(declared, assigned);
    }
    if (tg && cAssignContains(tg, r)) return r->nonUnion ? declared : cUnported("assignment to a member of a union");
    if (tg && tg->kind != TSK_Identifier && tg->kind != TSK_PropertyAccessExpression && tg->kind != TSK_ElementAccessExpression &&
        cMentionsText(tg, r->text, r->len))
      return cUnported("destructuring assignment of the reference");
    return cFlowWalkType(f->ante, r);
  }
  if (fl & (FF_TrueCondition | FF_FalseCondition)) {
    TsType *t = cFlowWalkType(f->ante, r);
    if (t == cErrorType) return t;
    return cNarrowByCondition(f->node, (fl & FF_TrueCondition) != 0, t, r);
  }
  if (fl & FF_SwitchClause) {
    TsType *t = cFlowWalkType(f->ante, r);
    if (t == cErrorType) return t;
    if (cMentionsText(f->node->expression, r->text, r->len)) return cUnported("switch narrowing");
    return t;
  }
  if (fl & FF_Call) {
    TsType *t = cFlowWalkType(f->ante, r);
    if (t == cErrorType) return t;
    bool argMentions = false;
    if (f->node->arguments)
      for (int i = 0; i < f->node->arguments->n; i++)
        if (cMentionsText(f->node->arguments->v[i], r->text, r->len)) argMentions = true;
    if (!argMentions) return t;
    bool known;
    bool pred = cCalleeHasPredicate(f->node, &known);
    if (known && !pred) return t;
    return cUnported("assertion call narrowing");
  }
  return cFlowWalkType(f->ante, r);
}

static TsType *cFlowWalkType(TsFlow *f, CFlowRef *r) {
  if (!f) return cUnported("flow node without antecedent");
  if (f->walkStamp == cFlowStamp) {
    if (f->walkDone) return f->walkType;
    /* in progress: only a loop label is revisited, and it gives the partial union so far */
    if (!(f->flags & FF_LoopLabel)) return cUnported("flow cycle");
    if (!f->walkRevisited) { f->walkRevisited = 1; f->walkSeen = f->walkType ? f->walkType : cNeverType; }
    else if (f->walkSeen != (f->walkType ? f->walkType : cNeverType)) f->walkMismatch = 1;
    return f->walkType ? f->walkType : cNeverType;
  }
  f->walkStamp = cFlowStamp;
  f->walkType = NULL;
  f->walkDone = 0;
  f->walkRevisited = 0;
  f->walkMismatch = 0;
  f->walkSeen = NULL;
  TsType *t = cFlowWalkNode(f, r);
  f->walkType = t;
  f->walkDone = 1;
  return t;
}

/* getFlowTypeOfReference: the declared type when no narrowing applies, else the narrowed type (ported forms only) */
static TsType *cFlowTypeOfReference(TsNode *ref, CFlowRef *r) {
  TsType *d = r->declared;
  if (!ref->flowNode || (d->flags & (TF_Undefined | TF_Null | TF_Void | TF_Never))) return d;
  cFlowStamp++;
  r->nonUnion = !(d->flags & TF_Union);
  return cFlowWalkType(ref->flowNode, r);
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
  /* checkIdentifier: a const variable of an enclosing container is narrowed through the function values that
   * create the reference (the flow container moves up past function expressions, arrows and object-literal methods) */
  bool jump = false;
  TsNode *flowC = NULL;
  TsNode *decl = cFirstDeclOf(s);
  if (outer && decl && decl->kind == TSK_VariableDeclaration && decl->parent && decl->parent->kind == TSK_VariableDeclarationList &&
      (decl->parent->flags & 7u) == TNF_Const) {
    TsNode *declC = cFlowContainerOf(decl);
    flowC = cFlowContainerOf(e);
    while (flowC && flowC != declC &&
           (flowC->kind == TSK_FunctionExpression || flowC->kind == TSK_ArrowFunction ||
            ((flowC->kind == TSK_MethodDeclaration || flowC->kind == TSK_GetAccessor || flowC->kind == TSK_SetAccessor) &&
             (bContainerFlags(flowC) & CF_IsObjectLiteralOrClassExpressionMethodOrAccessor))))
      flowC = cFlowContainerOf(flowC);
    jump = flowC != NULL;
  }
  CFlowRef r = { e, e->text, e->textLen, d, false, outer, startInit, jump, flowC };
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
  CFlowRef r = { e, text, len, declared, false, false, startInit, false, NULL };
  return cFlowTypeOfReference(e, &r);
}

/* ObjectFlags of an array literal type (createArrayLiteralType): the ArrayLiteral flag of a clone */
#define OF_ArrayLiteral 0x2000

/* ---- contextual types (getContextualType): NULL when there is none, the error type when it is not ported ---- */
static TsType *cContextualReturnOf(TsNode *fn);
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
      return cContextualReturnOf(fn);
    }
    case TSK_ArrowFunction:
      if (p->body != c) return cUnported("contextual type of an arrow parameter");
      return cContextualReturnOf(p);
    case TSK_AsExpression:
      if (p->expression != c) return cUnported("contextual type of an assertion type");
      return cTypeFromNode(p->type);
    case TSK_CallExpression: case TSK_NewExpression: {
      /* the parameter of a single non-generic signature (the candidates of several signatures are not ported) */
      if (!p->arguments) return cUnported("contextual type of a call");
      int idx = -1;
      for (int i = 0; i < p->arguments->n; i++) if (p->arguments->v[i] == c) idx = i;
      if (idx < 0) return cUnported("contextual type of a callee");
      TsType *f = cExprType(p->expression);
      if (f == cErrorType) return f;
      if (!(f->flags & TF_Object)) return cUnported("contextual type of a call of a non-object");
      cResolveSignatures(f);
      int ns = p->kind == TSK_NewExpression ? f->nconstruct : f->ncall;
      CSig *sigs = p->kind == TSK_NewExpression ? f->construct : f->call;
      if (ns != 1) return cUnported("contextual type of a call with several signatures");
      CSig *sig = &sigs[0];
      if (sig->decl->typeParameters && sig->decl->typeParameters->n) return cUnported("contextual type of a generic call");
      CParams ps;
      if (!cSplitParams(sig->decl, &ps) || ps.thisParam) return cUnported("contextual type of a call with a this parameter");
      bool ok;
      TsType *pt = cSigParamType(sig, &ps, idx, &ok);
      return ok ? pt : NULL;
    }
    case TSK_PropertyAssignment: {
      /* the property of the contextual type of the object literal */
      if (p->initializer != c || !p->name || p->name->kind != TSK_Identifier || !p->parent || p->parent->kind != TSK_ObjectLiteralExpression)
        return cUnported("contextual type of a property name");
      TsType *oc = cContextualType(p->parent);
      if (oc == NULL || oc == cErrorType) return oc;
      if (!(oc->flags & TF_Object)) return cUnported("contextual type of a property of a non-object");
      TsSymbol *ps = cPropertyOf(oc, bnEscapeText(p->name->text, p->name->textLen));
      return ps ? cTypeOfSymbol(ps) : NULL;
    }
    case TSK_ArrayLiteralExpression: {
      TsType *ac = cContextualType(p);
      if (ac == NULL || ac == cErrorType) return ac;
      if (!(ac->flags & TF_Object) || cIsTupleType(ac)) return cUnported("contextual element of a tuple or non-object");
      cResolveIndexInfos(ac);
      for (int i = 0; i < ac->nindex; i++) if (ac->index[i].keyType == cNumberType) return ac->index[i].type;
      return cUnported("contextual element without a number index");
    }
    case TSK_ConditionalExpression:
      if (p->condition == c) return NULL;
      return cContextualType(p);
    default: break;
  }
  return cUnported("contextual type");
}

/* getContextualSignature of a function expression or arrow: the one call signature of its contextual type that is not too
 * small for it (getContextualCallSignature with isAritySmaller). A union, an intersection or a context without signatures,
 * several signatures, a this parameter, a generic signature and every context not ported set *err (NULL with *err set),
 * and a NULL result without *err means there is no contextual type at all. */
static CSig *cContextualSignature(TsNode *fn, bool *err) {
  *err = false;
  TsType *ctx = cContextualType(fn);
  if (ctx == NULL) return NULL;
  if (ctx == cErrorType || !(ctx->flags & TF_Object)) { *err = true; return NULL; }
  cResolveSignatures(ctx);
  if (ctx->ncall == 0) { *err = true; return NULL; }
  /* isAritySmaller: the target parameters before the first optional, initialized or rest one (a this parameter is not counted) */
  TsList *pl = fn->parameters;
  int target = 0;
  while (pl && target < pl->n && !pl->v[target]->initializer && !pl->v[target]->questionToken && !pl->v[target]->dotDotDotToken) target++;
  if (pl && pl->n && cIsThisParam(pl->v[0])) target--;
  int applicable = 0;
  CSig *pick = NULL;
  for (int c = 0; c < ctx->ncall; c++) {
    CSig *s = &ctx->call[c];
    CParams sp;
    if (!cSplitParams(s->decl, &sp) || sp.thisParam) { *err = true; return NULL; }
    if (!sp.rest && sp.n < target) continue;
    applicable++;
    pick = s;
  }
  if (applicable != 1) { *err = true; return NULL; }
  if (pick->decl->typeParameters && pick->decl->typeParameters->n) { *err = true; return NULL; }
  return pick;
}

/* getContextualReturnType: the return annotation, else the return type of the contextual signature (function expressions and
 * arrows only; a function declaration has none). A generator, an async function and a method are not ported here. */
static TsType *cContextualReturnOf(TsNode *fn) {
  if (fn->type) {
    if (fn->asteriskToken || (bModifierFlags(fn) & TMF_Async)) return cUnported("contextual return of a generator or async function");
    return cTypeFromNode(fn->type);
  }
  if (fn->kind == TSK_FunctionDeclaration || fn->kind == TSK_Constructor || fn->kind == TSK_GetAccessor || fn->kind == TSK_SetAccessor) return NULL;
  if (fn->kind != TSK_FunctionExpression && fn->kind != TSK_ArrowFunction) return cUnported("contextual return type of a method");
  if (fn->asteriskToken || (bModifierFlags(fn) & TMF_Async)) return cUnported("contextual return of a generator or async function");
  bool err = false;
  CSig *cs = cContextualSignature(fn, &err);
  if (err) return cErrorType;
  return cs ? cReturnTypeOf(cs) : NULL;
}

/* a type with a literal or type variable member: a contextual type that keeps a literal return (isLiteralOfContextualType) */
static bool cHasLiteralOrVariable(TsType *t) {
  if (t->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BooleanLiteral | TF_BigIntLiteral | TF_TypeParameter |
                  TF_IndexedAccess | TF_Conditional | TF_Index | TF_TemplateLiteral | TF_StringMapping)) return true;
  if (t->flags & TF_Union) for (int i = 0; i < t->ntypes; i++) if (cHasLiteralOrVariable(t->types[i])) return true;
  return false;
}

/* getContextuallyTypedParameterType and assignContextualParameterTypes: an unannotated parameter of a function expression or
 * arrow has the type at its position in the contextual signature. Parameters with an initializer, optional or rest
 * parameters, binding patterns and the parameters of other functions (implicit any) are not ported. */
static TsType *cUnannotatedParamType(TsSymbol *sym, TsNode *d) {
  (void)sym;
  TsNode *fn = d->parent;
  if (!fn || (fn->kind != TSK_FunctionExpression && fn->kind != TSK_ArrowFunction)) return cUnported("parameter without annotation");
  if (d->initializer || d->questionToken || d->dotDotDotToken || !d->name || d->name->kind != TSK_Identifier)
    return cUnported("parameter without annotation");
  if (d->inferState == 1) return cUnported("circular parameter type");
  d->inferState = 1;
  TsType *t = cUnported("parameter without annotation");
  bool err = false;
  CSig *cs = cContextualSignature(fn, &err);
  if (!err && cs) {
    int idx = 0;
    while (idx < fn->parameters->n && fn->parameters->v[idx] != d) idx++;
    if (fn->parameters->n && cIsThisParam(fn->parameters->v[0])) idx--;
    CParams cp;
    if (idx >= 0 && cSplitParams(cs->decl, &cp)) {
      bool ok;
      TsType *pt = cSigParamType(cs, &cp, idx, &ok);
      /* a position beyond the contextual parameters has no type (tryGetTypeAtPosition): implicit any */
      if (ok) t = pt;
    }
  }
  d->inferState = 0;
  return t;
}

/* isLiteralOfContextualType: a fresh literal keeps its literal type when the contextual type has a literal of its kind */
static bool cLiteralOfContext(TsType *cand, TsType *ctx) {
  if (ctx->flags & TF_Union) {
    for (int i = 0; i < ctx->ntypes; i++) if (cLiteralOfContext(cand, ctx->types[i])) return true;
    return false;
  }
  if (ctx->flags & (TF_TypeParameter | TF_IndexedAccess | TF_Conditional | TF_Index)) return false;
  if (cand->flags & TF_StringLiteral) return (ctx->flags & (TF_StringLiteral | TF_TemplateLiteral | TF_StringMapping)) != 0;
  if (cand->flags & TF_NumberLiteral) return (ctx->flags & TF_NumberLiteral) != 0;
  if (cand->flags & TF_BooleanLiteral) return (ctx->flags & TF_BooleanLiteral) != 0;
  return false;
}

/* checkExpressionForMutableLocation: a fresh literal widens unless its contextual type has a literal of its kind */
static TsType *cMutableLocationType(TsNode *e, TsType *t) {
  if (t == cErrorType || !cIsFreshLiteral(t)) return cWidenLiteralType(t);
  TsType *ctx = cContextualType(e);
  if (ctx == cErrorType) return ctx;
  if (ctx == NULL) return cWidenLiteralType(t);
  return cLiteralOfContext(t, ctx) ? t : cWidenLiteralType(t);
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

/* getSignatureApplicabilityError: the arguments are related to the parameters in order; the first failure ends it */
static bool cArgsApplicable(CParams *ps, TsType **at, int na, CSig *sig, int rel) {
  for (int i = 0; i < na; i++) {
    bool ok;
    TsType *pt = cSigParamType(sig, ps, i, &ok);
    if (!ok || pt == cErrorType || at[i] == cErrorType) continue;
    if (!cIsRelatedWith(at[i], pt, rel)) return false;
  }
  return true;
}

/* ---- generic calls (resolveCall with inferTypeArguments and getSignatureInstantiation, TypeScript 5.9.2) ----
 * Ported subset: a call without type arguments and without a contextual type, whose arguments are not context
 * sensitive (their types come without a contextual type, see cContextualType), and whose generic candidates have no
 * this or rest parameter, no default type parameter, no const modifier, and constraints that do not mention type
 * parameters and are not primitive. Candidates keep the candidate order of chooseOverload; the inference of each
 * candidate runs again for each relation, as chooseOverload does. */

static const uint32_t cPrimitiveFlagMask = TF_String | TF_Number | TF_BigInt | TF_Boolean | TF_Enum | TF_StringLiteral |
  TF_NumberLiteral | TF_BooleanLiteral | TF_BigIntLiteral | TF_EnumLiteral | TF_ESSymbol | TF_UniqueESSymbol | TF_Void |
  TF_Undefined | TF_Null | TF_Index | TF_TemplateLiteral | TF_StringMapping;

/* maybeTypeOfKind(type, Primitive | Index | TemplateLiteral | StringMapping) */
static bool cMaybePrimitive(TsType *t, bool *unp) {
  if (t->flags & TF_Conditional) { *unp = true; return false; }
  if (t->flags & cPrimitiveFlagMask) return true;
  if (t->flags & (TF_Union | TF_Intersection)) {
    for (int i = 0; i < t->ntypes; i++) if (cMaybePrimitive(t->types[i], unp)) return true;
  }
  return false;
}

/* the generic signature instance of a declared signature for type arguments (getSignatureInstantiation, cached) */
static CSig *cGetSignatureInstance(CSig *base, TsType **args, int n) {
  uint32_t h = 2166136261u ^ (uint32_t)((uintptr_t)base->decl >> 4);
  for (int i = 0; i < n; i++) h = (h ^ (uint32_t)args[i]->id) * 16777619u;
  for (CSigInst *e = cSigInstTab[h & 1023]; e; e = e->next) {
    if (e->decl != base->decl || e->n != n) continue;
    bool same = true;
    for (int i = 0; i < n && same; i++) same = e->args[i] == args[i];
    if (same) return &e->sig;
  }
  CSigInst *e = ts_arena_alloc(sizeof(CSigInst));
  e->decl = base->decl;
  e->args = cCopyTypes(args, n);
  e->n = n;
  int ntp;
  TsType **tps = cSigTypeParams(base->decl, &ntp);
  e->sig.decl = base->decl;
  e->sig.isConstruct = base->isConstruct;
  e->sig.thisType = base->thisType;
  e->sig.mapper = cCreateTypeMapper(tps, args, n);
  e->sig.erased = true;
  e->sig.ptc = NULL;
  e->next = cSigInstTab[h & 1023];
  cSigInstTab[h & 1023] = e;
  return &e->sig;
}

/* isTypeParameterAtTopLevel for the unions and intersections of a parameter type (a conditional is unported) */
static bool cTypeParamAtTop(TsType *t, TsType *tp, bool *unp) {
  if (t == tp) return true;
  if (t->flags & TF_Conditional) { *unp = true; return false; }
  if (t->flags & (TF_Union | TF_Intersection)) {
    for (int i = 0; i < t->ntypes; i++) if (cTypeParamAtTop(t->types[i], tp, unp)) return true;
  }
  return false;
}

/* whether a type mentions a type parameter (the shapes that are not ported are unported) */
static bool cMentionsTypeParam(TsType *t, TsType *tp, bool *unp) {
  if (!cCouldContain(t)) return false;
  if (t == tp) return true;
  if (t->flags & (TF_Union | TF_Intersection)) {
    for (int i = 0; i < t->ntypes; i++) if (cMentionsTypeParam(t->types[i], tp, unp)) return true;
    return false;
  }
  if ((t->flags & TF_Object) && (t->objectFlags & OF_Reference)) {
    for (int i = 0; i < t->nargs; i++) if (cMentionsTypeParam(t->args[i], tp, unp)) return true;
    return false;
  }
  *unp = true;
  return false;
}

/* a type parameter with a const modifier (isConstTypeVariable) */
static bool cTypeParamIsConst(TsType *tp) {
  if (!tp->symbol) return false;
  for (int d = 0; d < tp->symbol->ndecls; d++)
    if (tp->symbol->decls[d]->kind == TSK_TypeParameter && (bModifierFlags(tp->symbol->decls[d]) & TMF_Const)) return true;
  return false;
}

/* getBaseTypeOfLiteralType for the literal types that are ported (bigint and enum literals are unported) */
static TsType *cBaseOfLiteral(TsType *t, bool *unp) {
  if (t->flags & (TF_BigIntLiteral | TF_EnumLiteral)) { *unp = true; return t; }
  if (t->flags & TF_StringLiteral) return cStringType;
  if (t->flags & TF_NumberLiteral) return cNumberType;
  if (t->flags & TF_BooleanLiteral) return cBooleanType;
  if (t->flags & TF_Union) {
    TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)(t->ntypes ? t->ntypes : 1));
    bool same = true;
    for (int i = 0; i < t->ntypes; i++) {
      v[i] = cBaseOfLiteral(t->types[i], unp);
      same = same && v[i] == t->types[i];
    }
    return same ? t : cGetUnionType(v, t->ntypes);
  }
  return t;
}

/* literalTypesWithSameBaseType */
static bool cLiteralTypesWithSameBase(TsType **types, int n, bool *unp) {
  TsType *common = NULL;
  for (int i = 0; i < n; i++) {
    TsType *t = types[i];
    if (t->flags & TF_Never) continue;
    TsType *base = cBaseOfLiteral(t, unp);
    if (!common) common = base;
    if (base == t || base != common) return false;
  }
  return true;
}

/* getSingleCommonSupertype: the strict supertype candidate, else the subtype one */
static TsType *cSingleCommonSupertype(TsType **types, int n) {
  TsType *cand = types[0];
  for (int i = 1; i < n; i++) if (cIsRelatedWith(cand, types[i], REL_StrictSubtype)) cand = types[i];
  bool all = true;
  for (int i = 0; i < n && all; i++) all = types[i] == cand || cIsRelatedWith(types[i], cand, REL_StrictSubtype);
  if (all) return cand;
  TsType *sup = types[0];
  for (int i = 1; i < n; i++) if (cIsRelatedWith(sup, types[i], REL_Subtype)) sup = types[i];
  return sup;
}

/* getCommonSupertype under strictNullChecks (the nullable members are set aside and added back with getNullableType) */
static TsType *cCommonSupertype(TsType **types, int n) {
  if (n == 1) return types[0];
  bool unp = false;
  TsType **prim = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
  bool changed = false;
  uint32_t combined = 0;
  for (int i = 0; i < n; i++) {
    TsType *t = types[i];
    if (t->flags & TF_Union) for (int k = 0; k < t->ntypes; k++) combined |= t->types[k]->flags;
    else combined |= t->flags;
    if (t->flags & TF_Union) {
      TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)(t->ntypes ? t->ntypes : 1));
      int m = 0;
      for (int k = 0; k < t->ntypes; k++) if (!(t->types[k]->flags & (TF_Undefined | TF_Null))) v[m++] = t->types[k];
      prim[i] = m == t->ntypes ? t : (m == 0 ? cNeverType : (m == 1 ? v[0] : cGetUnionType(v, m)));
    } else {
      prim[i] = (t->flags & (TF_Undefined | TF_Null)) ? cNeverType : t;
    }
    if (prim[i] != t) changed = true;
  }
  TsType *sup = cLiteralTypesWithSameBase(prim, n, &unp) ? cGetUnionType(prim, n) : cSingleCommonSupertype(prim, n);
  if (unp) return cUnported("common supertype of a bigint or enum literal");
  if (!changed) return sup;
  uint32_t missing = (combined & (TF_Undefined | TF_Null)) & ~sup->flags;
  if (!missing) return sup;
  TsType *v[3] = { sup, NULL, NULL };
  int k = 1;
  if (missing & TF_Undefined) v[k++] = cUndefinedType;
  if (missing & TF_Null) v[k++] = cNullType;
  return cGetUnionType(v, k);
}

/* inferTypeArguments and getInferredTypes for a call without a contextual type and with context-insensitive arguments
 * (the subset of the header comment). The arguments are the types of the arguments (cExprType). Returns false when the
 * subset does not cover the call; the reason is then reported as unported. */
static bool cInferCallTypeArgs(CSig *sig, TsType **at, int na, TsType **args) {
  int n;
  TsType **tps = cSigTypeParams(sig->decl, &n);
  CParams ps;
  if (!cSplitParams(sig->decl, &ps)) { cUnported("too many parameters"); return false; }
  if (ps.thisParam || ps.rest) { cUnported("generic call with a this or rest parameter"); return false; }
  for (int k = 0; k < n; k++) {
    if (cTypeParamDefaultNode(tps[k])) { cUnported("generic call with a default type parameter"); return false; }
    if (cTypeParamIsConst(tps[k])) { cUnported("generic call with a const type parameter"); return false; }
    TsType *c = cConstraintOf(tps[k]);
    if (c) {
      bool unp = false;
      if (cCouldContain(c)) { cUnported("generic call with a constraint of type parameters"); return false; }
      if (cMaybePrimitive(c, &unp) || unp) { cUnported("generic call with a primitive constraint"); return false; }
    }
  }
  int before = cUnportedCount;
  CInfer ci;
  cInferInit(&ci, tps, n);
  bool *notTop = ts_arena_zalloc(sizeof(bool) * (size_t)(n ? n : 1));
  for (int i = 0; i < na; i++) {
    bool ok;
    TsType *pt = cSigParamType(sig, &ps, i, &ok);
    if (!ok || pt == cErrorType || at[i] == cErrorType) { cUnported("generic call with an unported argument or parameter"); return false; }
    if (!cCouldContain(pt)) continue;
    bool unp = false;
    for (int k = 0; k < n; k++) {
      if (cMentionsTypeParam(pt, tps[k], &unp) && !cTypeParamAtTop(pt, tps[k], &unp)) notTop[k] = true;
    }
    if (unp) { cUnported("generic call with a parameter of an unported shape"); return false; }
    /* checkExpressionWithContextualType keeps a literal argument regular when its parameter has a literal of its kind */
    if ((at[i]->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BooleanLiteral)) && !(pt->flags & TF_TypeParameter) &&
        cLiteralOfContext(at[i], pt)) { cUnported("literal argument of a literal parameter type"); return false; }
    cInferFrom(&ci, at[i], pt);
  }
  if (cUnportedCount != before) return false;
  TsType *ret = NULL;
  for (int k = 0; k < n; k++) {
    TsType *inferred;
    if (ci.prio[k] >= 0 && (ci.prio[k] & 416)) { cUnported("inference with a combining priority"); return false; }
    if (ci.ctn[k]) { cUnported("contravariant candidates of a generic call"); return false; }
    TsType *c = cConstraintOf(tps[k]);
    if (ci.cn[k] == 0) {
      /* no candidate: the constraint, else unknown (getInferredType) */
      inferred = c ? c : cUnknownType;
    } else {
      TsType **cs = ci.cand[k];
      int nc = ci.cn[k];
      for (int i = 0; i < nc; i++) {
        TsType *t = cs[i];
        if (t->flags & TF_Union) {
          for (int m = 0; m < t->ntypes; m++) if (t->types[m]->flags & TF_Object && (t->types[m]->objectFlags & (OF_ObjectLiteral | OF_ArrayLiteral))) { cUnported("object or array literal candidate"); return false; }
        } else if ((t->flags & TF_Object) && (t->objectFlags & (OF_ObjectLiteral | OF_ArrayLiteral))) {
          cUnported("object or array literal candidate");
          return false;
        }
      }
      /* getCovariantInference: the literal types widen unless the parameter is at the top level of the return type */
      if (!ret) {
        if (sig->decl->type && sig->decl->type->kind == TSK_TypePredicate) { cUnported("generic call with a type predicate"); return false; }
        ret = cReturnTypeOf(sig);
        if (ret == cErrorType) return false;
      }
      bool unp = false;
      bool atTop = cTypeParamAtTop(ret, tps[k], &unp);
      if (unp) { cUnported("generic call with a conditional return type"); return false; }
      bool widen = !notTop[k] && !atTop;
      TsType **base = ts_arena_alloc(sizeof(TsType *) * (size_t)nc);
      for (int i = 0; i < nc; i++) base[i] = widen ? cWidenLiteralType(cs[i]) : cs[i];
      for (int i = 0; i < nc; i++) if (base[i] == cErrorType) return false;
      inferred = cCommonSupertype(base, nc);
      if (inferred == cErrorType) return false;
    }
    if (c && !cIsRelatedWith(inferred, c, REL_Assignable)) inferred = c;
    args[k] = inferred;
  }
  return true;
}

/* the type of a call or new expression of non-generic signatures (resolveCall: chooseOverload with the subtype
 * relation first when there are several candidates, then the assignable relation). The first applicable candidate
 * gives the return type. Generic candidates, this parameters, spread arguments and no applicable candidate are not
 * ported. */
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
  if (ns == 0) return cUnported("call without signatures");
  if (e->typeArguments) return cUnported("call with type arguments");
  int na = e->arguments ? e->arguments->n : 0;
  for (int i = 0; i < na; i++)
    if (e->arguments->v[i]->kind == TSK_SpreadElement) return cUnported("spread argument");
  TsType **at = ts_arena_alloc(sizeof(TsType *) * (size_t)(na ? na : 1));
  for (int i = 0; i < na; i++) at[i] = cExprType(e->arguments->v[i]);
  CParams *pss = ts_arena_alloc(sizeof(CParams) * (size_t)ns);
  bool *arityOk = ts_arena_alloc(sizeof(bool) * (size_t)ns);
  bool *generic = ts_arena_zalloc(sizeof(bool) * (size_t)ns);
  bool anyGeneric = false;
  for (int c = 0; c < ns; c++) {
    CSig *sig = &sigs[c];
    generic[c] = sig->decl->typeParameters && sig->decl->typeParameters->n > 0;
    if (!cSplitParams(sig->decl, &pss[c])) return cUnported("too many parameters");
    if (pss[c].thisParam) return cUnported("call of a signature with a this parameter");
    if (generic[c] && pss[c].rest) return cUnported("call of a generic signature with a rest parameter");
    anyGeneric = anyGeneric || generic[c];
    int mx = pss[c].rest ? 1 << 30 : pss[c].n;
    arityOk[c] = na >= pss[c].min && na <= mx;
  }
  /* the return type inference from a contextual type is not ported */
  if (anyGeneric && cContextualType(e) != NULL) return cUnported("contextual type of a generic call");
  /* the candidate that is applicable first (in declaration order, among those of the right arity) */
  int chosen = -1;
  CSig *chosenSig = NULL;
  for (int pass = 0; pass < 2 && chosen < 0; pass++) {
    if (pass == 0 && ns == 1) continue;   /* a single candidate is checked with the assignable relation only */
    int rel = pass == 0 ? REL_Subtype : REL_Assignable;
    for (int c = 0; c < ns && chosen < 0; c++) {
      if (!arityOk[c]) continue;
      if (!generic[c]) {
        if (cArgsApplicable(&pss[c], at, na, &sigs[c], rel)) { chosen = c; chosenSig = &sigs[c]; }
        continue;
      }
      /* chooseOverload: inferTypeArguments, then the instance is checked (getSignatureApplicabilityError) */
      int ntp = sigs[c].decl->typeParameters->n;
      TsType **targs = ts_arena_alloc(sizeof(TsType *) * (size_t)ntp);
      if (!cInferCallTypeArgs(&sigs[c], at, na, targs)) return cErrorType;
      CSig *inst = cGetSignatureInstance(&sigs[c], targs, ntp);
      if (cArgsApplicable(&pss[c], at, na, inst, rel)) { chosen = c; chosenSig = inst; }
    }
  }
  if (chosen < 0) return cUnported("no applicable signature");
  TsType *rt = cReturnTypeOf(chosenSig);
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
    TsType *t = cMutableLocationType(el, cExprType(el));
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

/* hasTypeFacts (getTypeFacts of a union is the union of its members' facts); *ok is false when a member is not ported */
static bool cHasTypeFacts(TsType *t, uint32_t facts, bool *ok) {
  uint32_t f = cTypeFacts(t, ok);
  return (f & facts) != 0;
}

/* getDefinitelyFalsyPartOfType for one member: the member when it is definitely falsy, its falsy literal for string,
 * number and boolean types, never otherwise; bigint and enum members are not ported */
static TsType *cDefinitelyFalsyPart(TsType *m) {
  uint32_t f = m->flags;
  if (f & (TF_BigInt | TF_BigIntLiteral | TF_Enum | TF_EnumLiteral)) return cUnported("definitely falsy part of a bigint or enum");
  if (f & TF_String) return cLiteralType(TF_StringLiteral, "", 0);
  if (f & TF_Number) {
    TsType *z = cLiteralType(TF_NumberLiteral, "0", 1);
    z->litNum = 0;
    return z;
  }
  if (m == cFalseType) return m;
  if (f & (TF_Void | TF_Undefined | TF_Null | TF_Any | TF_Unknown)) return m;
  if ((f & TF_StringLiteral) && m->textLen == 0) return m;
  if ((f & TF_NumberLiteral) && m->litNum == 0) return m;
  return cNeverType;
}

/* extractDefinitelyFalsyTypes (mapType with literal reduction) */
static TsType *cExtractDefinitelyFalsy(TsType *t) {
  if (t == cErrorType) return t;
  int n = cMemberCount(t);
  TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  for (int i = 0; i < n; i++) {
    v[i] = cDefinitelyFalsyPart(cMemberAt(t, i));
    if (v[i] == cErrorType) return cErrorType;
  }
  return n == 1 ? v[0] : cGetUnionType(v, n);
}

/* getNonNullableType under strictNullChecks (getAdjustedTypeWithFacts with NEUndefinedOrNull, for the ported members) */
static TsType *cNonNullableType(TsType *t) {
  if (t == cErrorType) return t;
  if (t->flags & (TF_Any | TF_Unknown)) return cUnported("non-nullable any or unknown");
  return cTypeWithFacts(t, TFC_NEUndefinedOrNull);
}

/* getUnionType(types, UnionReduction.Subtype) of the members of two types, sorted by id as the type set is */
static TsType *cSubtypeUnionOfTwo(TsType *a, TsType *b) {
  int na = cMemberCount(a), nb = cMemberCount(b);
  if (a == cNeverType) na = 0;
  if (b == cNeverType) nb = 0;
  TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)(na + nb + 1));
  int n = 0;
  for (int i = 0; i < na; i++) v[n++] = cMemberAt(a, i);
  for (int i = 0; i < nb; i++) v[n++] = cMemberAt(b, i);
  for (int i = 1; i < n; i++) {
    TsType *x = v[i];
    int j = i - 1;
    while (j >= 0 && v[j]->id > x->id) { v[j + 1] = v[j]; j--; }
    v[j + 1] = x;
  }
  return n == 0 ? cNeverType : cUnionWithSubtypesRemoved(v, n);
}

/* checkBinaryLikeExpressionWorker for && || ??, under strictNullChecks (lines 84487 and after of TypeScript 5.9.2) */
static TsType *cLogicalResult(int op, TsType *l, TsType *r) {
  bool ok = true, has;
  if (op == TSK_AmpersandAmpersandToken) {
    has = cHasTypeFacts(l, TFC_Truthy, &ok);
    if (!ok) return cUnported("&& of a type that is not ported");
    if (!has) return l;
    TsType *falsy = cExtractDefinitelyFalsy(l);
    if (falsy == cErrorType) return falsy;
    return cGetUnionType((TsType *[]){ falsy, r }, 2);
  }
  if (op == TSK_BarBarToken) {
    has = cHasTypeFacts(l, TFC_Falsy, &ok);
    if (!ok) return cUnported("|| of a type that is not ported");
    if (!has) return l;
    TsType *a = cTypeWithFacts(l, TFC_Truthy);
    if (a == cErrorType) return a;
    a = cNonNullableType(a);
    if (a == cErrorType) return a;
    return cSubtypeUnionOfTwo(a, r);
  }
  /* ?? */
  has = cHasTypeFacts(l, TFC_EQUndefinedOrNull, &ok);
  if (!ok) return cUnported("?? of a type that is not ported");
  if (!has) return l;
  TsType *a = cNonNullableType(l);
  if (a == cErrorType) return a;
  return cSubtypeUnionOfTwo(a, r);
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
    case TSK_AmpersandAmpersandToken: case TSK_BarBarToken: case TSK_QuestionQuestionToken: {
      TsType *l = cExprType(e->left), *r = cExprType(e->right);
      if (l == cErrorType || r == cErrorType) return cUnported("logical operator of an unported operand");
      return cLogicalResult(op, l, r);
    }
    case TSK_PlusEqualsToken: case TSK_MinusEqualsToken: case TSK_AsteriskEqualsToken: case TSK_SlashEqualsToken:
    case TSK_PercentEqualsToken: case TSK_LessThanLessThanEqualsToken: case TSK_GreaterThanGreaterThanEqualsToken:
    case TSK_GreaterThanGreaterThanGreaterThanEqualsToken: case TSK_AmpersandEqualsToken: case TSK_BarEqualsToken:
    case TSK_CaretEqualsToken: {
      TsType *l = cExprType(e->left), *r = cExprType(e->right);
      if (l == cErrorType || r == cErrorType) return cUnported("compound assignment of an unported operand");
      TsType *res;
      if (op == TSK_PlusEqualsToken) {
        if (cIsNumberLike(l) && cIsNumberLike(r)) res = cNumberType;
        else if (cIsStringLike(l) || cIsStringLike(r)) res = cStringType;
        else return cUnported("compound sum of other types");
      } else {
        if ((l->flags | r->flags) & TF_BigInt) return cUnported("compound arithmetic on bigint");
        res = cNumberType;
      }
      /* checkAssignmentOperator: the result is related to the target, which is the declared type */
      TsType *lt = cAssignmentTargetType(e->left);
      if (lt != cErrorType && !cIsRelatedWith(res, lt, REL_Assignable)) cDiagAdd(cSourceFileOf(e->left), cNodeStart(e->left), 2322, NULL);
      return res;
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
    case TSK_ArrayLiteralExpression: {
      TsType *ac = cContextualType(e);
      if (ac == cErrorType) return ac;
      if (ac != NULL && ((ac->flags & TF_Object) == 0 || cIsTupleType(ac))) return cUnported("array literal of a tuple or non-object context");
      return cArrayLiteralType(e);
    }
    case TSK_ObjectLiteralExpression: {
      TsType *oc = cContextualType(e);
      if (oc == cErrorType) return oc;
      return cObjectLiteralType(e);
    }
    case TSK_FunctionExpression: case TSK_ArrowFunction: {
      if (!e->symbol) return cUnported("function expression without symbol");
      if (cContextualType(e) != NULL) {
        /* checkFunctionExpressionOrObjectLiteralMethod: the parameters take the contextual signature (cUnannotatedParamType);
         * a literal return of the body is kept when the contextual return type has a literal or type variable */
        bool err = false;
        CSig *cs = cContextualSignature(e, &err);
        if (err || !cs) return cUnported("function expression with a contextual type");
        if (!e->type) {
          TsType *cr = cReturnTypeOf(cs);
          if (cr == cErrorType) return cr;
          if (cHasLiteralOrVariable(cr)) {
            TsType *rt = cInferReturnType(e);
            if (rt == cErrorType) return rt;
            if (e->retLiteral) return cUnported("literal return of a contextually typed function");
          }
        }
      }
      return cTypeOfSymbol(e->symbol);
    }
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
