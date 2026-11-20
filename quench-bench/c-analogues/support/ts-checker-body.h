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
  TsType *initial;       /* initialType of the walk (the declared type, or its optional type when not assumed initialized) */
  int depth;             /* flowDepth of the walk */
  int sharedStart;       /* the shared flow memo of this walk starts here (sharedFlowStart) */
  const char *key;       /* the flow cache key of the reference (getFlowCacheKey), built when a loop label needs it */
  bool keyDone;
} CFlowRef;


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

/* the type of a reference at the start of its container (getTypeAtFlowNode, FlowStart): initialType. An outer variable is
 * assumed initialized, so its initial type is its declared type; a variable of the container needs startInit. */
static TsType *cFlowStartType(CFlowRef *r) {
  if (r->outer) return r->declared;
  return r->startInit ? r->declared : r->initial;   /* an uninitialized reference starts at its optional type (getOptionalType) */
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

/* getNarrowedType (75764) with checkDerived false, as type predicates use it. Assuming the predicate holds, each member of
 * the candidate narrows the members of the type that are strict subtypes or subtypes of it; a type that is related to no
 * member is replaced by the candidate, the type or the intersection. When it is false, the members that are subsets of the
 * narrowed true type are kept. The key property of a large union (10 members or more) and instantiable members are not
 * ported; a relation that meets an unported path makes the result unported. */
static TsType *cGetNarrowedType(TsType *type, TsType *cand, bool assumeTrue);

static bool cContainsMember(TsType *u, TsType *x) {
  if (!(u->flags & TF_Union)) return false;
  for (int i = 0; i < u->ntypes; i++) if (u->types[i] == x) return true;
  return false;
}

static bool cHasInstantiableMember(TsType *t) {
  for (int i = 0, n = cMemberCount(t); i < n; i++)
    if (cMemberAt(t, i)->flags & (TF_TypeParameter | TF_Index | TF_IndexedAccess | TF_Conditional | TF_TemplateLiteral | TF_StringMapping))
      return true;
  return false;
}

/* the members of t that are mapped by f (a NULL result is dropped, never is dropped); the result is the union of them */
static TsType *cNarrowMembers(TsType *t, TsType *c, bool trueBranch, TsType *tr) {
  int n = cMemberCount(t);
  TsType **res = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  int k = 0;
  for (int i = 0; i < n; i++) {
    TsType *m = cMemberAt(t, i);
    if (trueBranch) {
      /* getNarrowedType, true: the directly related member, or the candidate, or the instantiable intersection */
      TsType *d = cNeverType;
      if (cIsRelatedWith(m, c, REL_StrictSubtype)) d = m;
      else if (cIsRelatedWith(c, m, REL_StrictSubtype)) d = c;
      else if (cIsRelatedWith(m, c, REL_Subtype)) d = m;
      else if (cIsRelatedWith(c, m, REL_Subtype)) d = c;
      if (d->flags & TF_Never) {
        /* node takes the instantiable fallback only when no member is related directly: unported here */
        if (cHasInstantiableMember(t)) return cUnported("predicate narrowing of an instantiable member");
        continue;
      }
      res[k++] = d;
    } else {
      /* getNarrowedType, false: the members that are not subsets of the narrowed true type */
      if (!cContainsMember(tr, m) && m != tr) res[k++] = m;
    }
  }
  if (k == 0) return cNeverType;
  return k == 1 ? res[0] : cGetUnionType(res, k);
}

static TsType *cGetNarrowedType(TsType *type, TsType *cand, bool assumeTrue) {
  if (type == cErrorType || cand == cErrorType) return type == cErrorType ? type : cUnported("predicate type that is not ported");
  int before = cUnportedCount;
  TsType *r;
  if (!assumeTrue) {
    if (type == cand) return cNeverType;
    if (type->flags & TF_Unknown) return cUnported("predicate narrowing of unknown, false branch");
    TsType *trueT = cGetNarrowedType(type, cand, true);
    if (trueT == cErrorType) return cUnported("predicate narrowing of the true branch");
    r = (type->flags & TF_Never) ? type : cNarrowMembers(type, NULL, false, trueT);
  } else {
    if (type->flags & (TF_Any | TF_Unknown)) return cand;
    if (type == cand) return cand;
    if ((type->flags & TF_Union) && type->ntypes >= 10) return cUnported("predicate narrowing of a large union (key property)");
    int nc = cMemberCount(cand);
    TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(nc ? nc : 1));
    int np = 0;
    for (int i = 0; i < nc; i++) {
      TsType *c = cMemberAt(cand, i);
      TsType *d = cNarrowMembers(type, c, true, NULL);
      if (d == cErrorType) return d;
      if (d->flags & TF_Never) {
        if (cUnportedCount != before) return cUnported("predicate narrowing that meets an unported relation");
        d = cNeverType;
      }
      if (!(d->flags & TF_Never)) parts[np++] = d;
    }
    r = np == 0 ? cNeverType : np == 1 ? parts[0] : cGetUnionType(parts, np);
    if (r->flags & TF_Never) {
      if (cIsRelatedWith(cand, type, REL_Subtype)) r = cand;
      else if (cIsRelatedWith(type, cand, REL_Assignable)) r = type;
      else if (cIsRelatedWith(cand, type, REL_Assignable)) r = cand;
      else r = cGetIntersectionType((TsType *[]){ type, cand }, 2);
    }
  }
  if (cUnportedCount != before) return cUnported("predicate narrowing that meets an unported relation");
  return r;
}

/* narrowTypeByTypePredicate: a call with one signature whose return is a type predicate `x is T` of an identifier parameter,
 * where the reference is that argument (getNarrowedType: the predicate type when true, the reference type when false, with
 * the members of the reference narrowed as cGetNarrowedType does). */
static TsType *cNarrowByPredicate(TsNode *c, bool truthy, TsType *t, CFlowRef *r) {
  TsType *f = cExprType(c->expression);
  if (f == cErrorType || !(f->flags & TF_Object)) return cUnported("narrowing by a call");
  cResolveSignatures(f);
  if (f->ncall != 1) return cUnported("narrowing by a call");
  TsNode *d = f->call[0].decl;
  TsNode *pred = d->type;
  if ((d->typeParameters && d->typeParameters->n) || !pred || pred->assertsModifier || !pred->type || !pred->parameterName ||
      pred->parameterName->kind != TSK_Identifier) return cUnported("narrowing by a call");
  CParams ps;
  if (!cSplitParams(d, &ps) || ps.thisParam || ps.rest) return cUnported("narrowing by a call");
  int idx = -1;
  for (int k = 0; k < ps.n; k++) {
    TsNode *pn = ps.params[k]->name;
    if (pn && pn->kind == TSK_Identifier && pn->textLen == pred->parameterName->textLen &&
        memcmp(pn->text, pred->parameterName->text, (size_t)pn->textLen) == 0) idx = k;
  }
  if (idx < 0 || !c->arguments || idx >= c->arguments->n) return cUnported("narrowing by a call");
  /* narrowTypeByTypePredicate: an argument that is not the reference leaves the type, unless it is a discriminant of a union
   * (not ported) */
  TsNode *arg = c->arguments->v[idx];
  if (!cMatchesRefTree(cSkipParens(arg), r)) {
    if ((t->flags & TF_Union) && cAssignContains(arg, r)) return cUnported("discriminant narrowing by a predicate");
    return t;
  }
  TsType *cand = cTypeFromNode(pred->type);
  if (cand == cErrorType || (cand->flags & (TF_Any | TF_Unknown))) return cUnported("narrowing by a call");
  if (t == cErrorType) return t;
  /* narrowTypeByTypePredicate: an any reference is kept for the global Object and Function types */
  if ((t->flags & TF_Any) && (cand->flags & TF_Object) && cand->symbol) {
    TsSymbol *sym = cand->symbol;
    bool globalish = (sym->name.n == 6 && memcmp(sym->name.s, "Object", 6) == 0) ||
                     (sym->name.n == 8 && memcmp(sym->name.s, "Function", 8) == 0);
    if (globalish) return cUnported("predicate of the global Object or Function type");
  }
  return cGetNarrowedType(t, cand, truthy);
}

/* isCoercibleUnderDoubleEquals (5.9.2, 72371) */
static bool cCoercibleUnderDoubleEquals(TsType *s, TsType *t) {
  return (s->flags & (TF_Number | TF_String | TF_BooleanLiteral)) != 0 && (t->flags & (TF_Number | TF_String | TF_Boolean)) != 0;
}

/* isUnitLikeType: a unit type, or an intersection with one; the instantiable members are not ported */
static bool cIsUnitLikeMember(TsType *m, bool *ok) {
  if (m->flags & TF_Intersection) {
    for (int i = 0; i < m->ntypes; i++) if (cIsUnitType(m->types[i])) return true;
    return false;
  }
  if (m->flags & (TF_TypeParameter | TF_Index | TF_IndexedAccess | TF_Conditional | TF_TemplateLiteral | TF_StringMapping)) *ok = false;
  return cIsUnitType(m);
}

/* filterType with the comparability test of a value: the members for which keep holds (the type itself when none is
 * removed, never when none is kept). unit: the test is isUnitLikeType(member) && comparable, negated. */
static TsType *cFilterByValue(TsType *t, TsType *value, bool doubleEq, bool unitRemoval, bool *ok) {
  if (t->flags & TF_Never) return t;
  int n = cMemberCount(t);
  TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  int k = 0;
  for (int i = 0; i < n; i++) {
    TsType *m = cMemberAt(t, i);
    bool keep;
    if (unitRemoval) keep = !(cIsUnitLikeMember(m, ok) && cAreTypesComparable(m, value));
    else keep = cAreTypesComparable(m, value) || (doubleEq && cCoercibleUnderDoubleEquals(m, value));
    if (keep) kept[k++] = m;
  }
  if (!(t->flags & TF_Union)) return k ? t : cNeverType;
  if (k == n) return t;
  if (k == 0) return cNeverType;
  return k == 1 ? kept[0] : cGetUnionType(kept, k);
}

/* replacePrimitivesWithLiterals: a string, number or bigint member of the filtered type becomes the literals of the
 * value's type of the same kind (the template and string mapping forms are not ported) */
static TsType *cReplacePrimitivesWithLiterals(TsType *withPrim, TsType *withLit) {
  int np = cMemberCount(withPrim), nl = cMemberCount(withLit);
  bool primMaybe = false, litMaybe = false;
  for (int i = 0; i < np; i++) {
    TsType *m = cMemberAt(withPrim, i);
    if (m->flags & (TF_TemplateLiteral | TF_StringMapping)) return cUnported("template literal in an equality narrowing");
    if (m->flags & (TF_String | TF_Number | TF_BigInt)) primMaybe = true;
  }
  for (int i = 0; i < nl; i++) {
    TsType *m = cMemberAt(withLit, i);
    if (m->flags & (TF_TemplateLiteral | TF_StringMapping)) return cUnported("template literal in an equality narrowing");
    if (m->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral)) litMaybe = true;
  }
  if (!(primMaybe && litMaybe)) return withPrim;
  TsType **res = ts_arena_alloc(sizeof(TsType *) * (size_t)(np ? np : 1));
  int k = 0;
  bool changed = false;
  for (int i = 0; i < np; i++) {
    TsType *m = cMemberAt(withPrim, i);
    uint32_t want = (m->flags & TF_String) ? (TF_String | TF_StringLiteral) : (m->flags & TF_Number) ? (TF_Number | TF_NumberLiteral)
                  : (m->flags & TF_BigInt) ? (TF_BigInt | TF_BigIntLiteral) : 0;
    if (!want) { res[k++] = m; continue; }
    changed = true;
    for (int j = 0; j < nl; j++) {
      TsType *x = cMemberAt(withLit, j);
      if (x->flags & want) res[k++] = x;   /* extractTypesOfKind: the members of the value's type of that kind */
    }
  }
  if (!changed) return withPrim;
  if (k == 0) return cNeverType;
  return k == 1 ? res[0] : cGetUnionType(res, k);
}

/* narrowTypeByEquality (5.9.2, 75503) against a value that is neither undefined nor null: the true branch keeps the
 * members comparable with the value (or coercible under ==) and replaces primitives with the value's literals; the
 * false branch removes the members that are the unit value, when the value is a unit type. */
static TsType *cNarrowByEqualityValue(TsType *t, TsType *value, bool doubleEq, bool assumeTrue) {
  if (value == cErrorType) return cUnported("equality with an unported value");
  if (assumeTrue) {
    if (!doubleEq) {
      bool unknownOrEmpty = (t->flags & TF_Unknown) != 0;
      for (int i = 0; i < cMemberCount(t) && !unknownOrEmpty; i++) unknownOrEmpty = cIsEmptyAnonymousObject(cMemberAt(t, i));
      if (unknownOrEmpty) return cUnported("equality narrowing of unknown or an empty object type");
    }
    int before = cUnportedCount;
    bool ok = true;
    TsType *filtered = cFilterByValue(t, value, doubleEq, false, &ok);
    if (!ok || cUnportedCount != before) return cUnported("comparability in an equality narrowing");
    return cReplacePrimitivesWithLiterals(filtered, value);
  }
  if (!cIsUnitType(value)) return t;
  int before = cUnportedCount;
  bool ok = true;
  TsType *res = cFilterByValue(t, value, false, true, &ok);
  if (!ok || cUnportedCount != before) return cUnported("comparability in an equality narrowing");
  return res;
}

/* narrowTypeByInstanceof (75714) for a reference of type any: the instance type of the constructor when the test holds, the
 * reference type otherwise. The right side must be an object derived from Object and Function (not an empty anonymous
 * type); a hasInstance member, or a constructor without a prototype property, is not ported. */
static TsType *cNarrowByInstanceof(TsNode *c, bool truthy, TsType *t) {
  TsType *rt = cExprType(c->right);
  if (rt == cErrorType) return cUnported("instanceof of an unported constructor");
  if (rt->flags & TF_Any) return t;
  if (!(rt->flags & TF_Object)) return cUnported("instanceof of a type that is not an object");
  if (cIsEmptyAnonymousObject(rt)) return t;
  TsSymbol **props;
  int np = cPropertiesOf(rt, &props);
  for (int p = 0; p < np; p++) {
    BName nm = props[p]->name;
    if (nm.n > 15 && memcmp(nm.s, "__@hasInstance@", 15) == 0) return cUnported("instanceof with a hasInstance member");
  }
  cResolveSignatures(rt);
  if (!rt->ncall && !rt->nconstruct) {
    if (cPropertyOf(rt, bnC("bind"))) return cUnported("instanceof of an object with a bind member");
    return t;   /* isFunctionObjectType is false */
  }
  TsSymbol *pr = cPropertyOf(rt, bnC("prototype"));
  if (!pr) return cUnported("instance type of a constructor without a prototype property");
  TsType *inst = cTypeOfSymbol(pr);
  if (inst == cErrorType) return inst;
  if (inst->flags & TF_Any) return cUnported("instance type of a constructor with an any prototype");
  bool instIsObject = (inst->flags & TF_Object) && !cIsEmptyAnonymousObject(inst);
  if (!truthy && !instIsObject) return t;
  /* narrowTypeByInstanceof keeps an any reference for the global Object and Function types */
  if (inst->flags & TF_Object && inst->symbol) {
    TsSymbol *sym = inst->symbol;
    if ((sym->name.n == 6 && memcmp(sym->name.s, "Object", 6) == 0) || (sym->name.n == 8 && memcmp(sym->name.s, "Function", 8) == 0))
      return cUnported("instanceof of the global Object or Function type");
  }
  /* getNarrowedType of an any reference: the candidate when true; the reference when false (filtering keeps any) */
  return truthy ? inst : t;
}

static TsSymbol *cUnionProperty(TsType *u, BName name);   /* the property of a union (below) */

/* isDiscriminantProperty (74019) of a union: the property of every member has a type that differs from the others and
 * includes a literal type, and the union property type is not generic. Returns 1, 0, or -1 when not ported (a member
 * without the property may have an index signature or be an object literal, which node types as a partial property). */
static int cIsDiscriminantProp(TsType *u, BName name) {
  if (!(u->flags & TF_Union)) return 0;
  TsType **pts = ts_arena_alloc(sizeof(TsType *) * (size_t)u->ntypes);
  bool nonUniform = false, hasLiteral = false;
  for (int i = 0; i < u->ntypes; i++) {
    TsType *m = u->types[i];
    if (!(m->flags & TF_Object)) return -1;
    TsSymbol *p = cPropertyOf(m, name);
    if (!p) return -1;
    TsType *pt = cTypeOfSymbol(p);
    if (pt == cErrorType) return -1;
    if (pt->flags & (TF_TemplateLiteral | TF_StringMapping)) return -1;
    pts[i] = pt;
    if (i && pt != pts[0]) nonUniform = true;
    bool lit = (pt->flags & TF_Boolean) != 0;
    if (!lit && (pt->flags & TF_Union)) {
      lit = true;
      for (int j = 0; j < pt->ntypes && lit; j++) lit = cIsUnitType(pt->types[j]);
    } else if (!lit) lit = cIsUnitType(pt);
    hasLiteral |= lit;
  }
  if (!nonUniform || !hasLiteral) return 0;
  return cIsGenericType(cGetUnionType(pts, u->ntypes)) ? 0 : 1;
}

/* the object of a discriminant access (getDiscriminantPropertyAccess, 75255): an access of the reference whose property is a
 * discriminant of the flow type of the reference, or of its declared union when the flow type is one of its members. NULL
 * when there is none; *unported when the answer is not ported (an optional chain, an element access without a literal). */
static TsNode *cDiscriminantAccess(TsNode *expr, CFlowRef *r, TsType *t, bool *unported) {
  TsNode *a = cSkipParens(expr);
  *unported = false;
  if (!a || (a->kind != TSK_PropertyAccessExpression && a->kind != TSK_ElementAccessExpression)) return NULL;
  if (!cMatchesRefTree(a->expression, r)) return NULL;
  bool declUnion = r->declared && (r->declared->flags & TF_Union);
  if (!declUnion && !(t->flags & TF_Union)) return NULL;
  BName name;
  if (a->kind == TSK_PropertyAccessExpression) {
    if (!a->name || a->name->kind != TSK_Identifier) { *unported = true; return NULL; }
    name = bnEscapeText(a->name->text, a->name->textLen);
  } else {
    TsNode *k = cSkipParens(a->argumentExpression);
    if (!k || (k->kind != TSK_StringLiteral && k->kind != TSK_NumericLiteral)) { *unported = true; return NULL; }
    if (k->kind == TSK_StringLiteral) name = bnEscapeText(k->text, k->textLen);
    else {
      double v = cNumericLiteralValue(k->text, k->textLen);
      char buf[64];
      int n = (v == (double)(long long)v) ? snprintf(buf, sizeof buf, "%lld", (long long)v) : snprintf(buf, sizeof buf, "%.17g", v);
      name = bnEscapeText(buf, n);
    }
  }
  TsType *type = t;
  if (declUnion && t != r->declared) {
    for (int i = 0; i < r->declared->ntypes; i++) if (r->declared->types[i] == t) { type = r->declared; break; }
  }
  int d = cIsDiscriminantProp(type, name);
  if (d < 0) { *unported = true; return NULL; }
  return d ? a : NULL;
}

/* the property name of a discriminant access (getAccessedPropertyName for the access nodes of cDiscriminantAccess) */
static BName cDiscriminantName(TsNode *a) {
  if (a->kind == TSK_PropertyAccessExpression) return bnEscapeText(a->name->text, a->name->textLen);
  TsNode *k = cSkipParens(a->argumentExpression);
  if (k->kind == TSK_StringLiteral) return bnEscapeText(k->text, k->textLen);
  double v = cNumericLiteralValue(k->text, k->textLen);
  char buf[64];
  int n = (v == (double)(long long)v) ? snprintf(buf, sizeof buf, "%lld", (long long)v) : snprintf(buf, sizeof buf, "%.17g", v);
  return bnEscapeText(buf, n);
}

/* narrowTypeByDiscriminant (75270): the members of the union whose property of the name is comparable with the narrowed
 * property type. narrow maps the property type (the equality or the truthiness of the access). Returns NULL when not ported. */
static TsType *cNarrowByDiscriminantWith(TsType *t, BName name, TsType *(*narrow)(TsType *, void *), void *ctx) {
  TsSymbol *up = cUnionProperty(t, name);
  if (!up) return NULL;
  TsType *np = narrow(cTypeOfSymbol(up), ctx);
  if (np == cErrorType) return np;
  int n = t->ntypes;
  TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
  int k = 0;
  for (int i = 0; i < n; i++) {
    TsType *m = t->types[i];
    TsSymbol *mp = cPropertyOf(m, name);
    TsType *dt = mp ? cTypeOfSymbol(mp) : cUnknownType;
    if (dt == cErrorType) return cErrorType;
    if (!(dt->flags & TF_Never) && !(np->flags & TF_Never) && cAreTypesComparable(np, dt)) kept[k++] = m;
  }
  if (k == n) return t;
  if (k == 0) return cNeverType;
  return k == 1 ? kept[0] : cGetUnionType(kept, k);
}

/* the discriminant narrowing of the equality of an access with a value (narrowTypeByDiscriminantProperty for the
 * operators other than the key property ones, and narrowTypeByEquality of the property type). */
typedef struct { TsType *value; bool doubleEq; bool holds; } CDiscEq;
static TsType *cDiscEqNarrow(TsType *pt, void *c) {
  CDiscEq *e = c;
  if (pt->flags & TF_Any) return pt;
  if (e->value == cErrorType) return cUnported("equality with an unported value");
  if (e->value->flags & (TF_Undefined | TF_Null)) return cNullishEquality(pt, e->value, e->doubleEq, e->holds);
  return cNarrowByEqualityValue(pt, e->value, e->doubleEq, e->holds);
}

/* the truthiness of a discriminant access (narrowTypeByTruthiness: getTypeWithFacts of the property type) */
typedef struct { bool truthy; } CDiscTruthy;
static TsType *cDiscTruthyNarrow(TsType *pt, void *c) {
  CDiscTruthy *d = c;
  if (pt == cErrorType || (pt->flags & TF_Any)) return pt;
  return cTypeWithFacts(pt, d->truthy ? TFC_Truthy : TFC_Falsy);
}

/* the key property path of narrowTypeByDiscriminantProperty (getKeyPropertyName) needs a union of ten or more object members;
 * it is not ported */
static bool cHasKeyPropertyCandidate(TsType *t) {
  if (!(t->flags & TF_Union) || t->ntypes < 10) return false;
  int objs = 0;
  for (int i = 0; i < t->ntypes; i++) if (t->types[i]->flags & (TF_Object | TF_Intersection | TF_TypeParameter | TF_IndexedAccess | TF_Conditional)) objs++;
  return objs >= 10;
}

enum { CE_None = 0, CE_Sig, CE_Unported };
static int cGetEffects(TsNode *call, TsNode **decl);   /* the effects signature of a call, below */

/* hasMatchingArgument: an argument is the reference or contains it (x.y for x), or the callee's object is */
static bool cHasMatchingArgument(TsNode *call, CFlowRef *r) {
  if (call->arguments)
    for (int i = 0; i < call->arguments->n; i++) {
      TsNode *a = call->arguments->v[i];
      if (cMatchesRefTree(cSkipParens(a), r) || cAssignContains(a, r)) return true;
    }
  TsNode *ce = call->expression;
  if (ce->kind == TSK_PropertyAccessExpression && (cMatchesRefTree(cSkipParens(ce->expression), r) || cAssignContains(ce->expression, r)))
    return true;
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
  if (c->kind == TSK_NonNullExpression || c->kind == TSK_SatisfiesExpression) return cNarrowByCondition(c->expression, truthy, t, r);
  if (c->kind == TSK_BinaryExpression) {
    int op = c->operatorToken->kind;
    bool isEq = op == TSK_EqualsEqualsEqualsToken || op == TSK_EqualsEqualsToken;
    bool isNe = op == TSK_ExclamationEqualsEqualsToken || op == TSK_ExclamationEqualsToken;
    /* narrowTypeByBinaryExpression: the operators other than the equalities, instanceof and in do not narrow */
    if (!isEq && !isNe && op != TSK_InstanceOfKeyword && op != TSK_InKeyword) {
      if (op == TSK_EqualsToken) return cUnported("assignment condition");
      return t;
    }
    /* instanceof narrows its left reference; in narrows its right reference (narrowTypeByInKeyword keeps any) */
    if (op == TSK_InstanceOfKeyword) {
      if (!cMatchesRefTree(cSkipParens(c->left), r)) return t;
      if (!(t->flags & TF_Any)) return cUnported("instanceof narrowing of a reference that is not any");
      return cNarrowByInstanceof(c, truthy, t);
    }
    if (op == TSK_InKeyword) {
      if (!cMatchesRefTree(cSkipParens(c->right), r)) return t;
      if (!(t->flags & TF_Any)) return cUnported("in narrowing of a reference that is not any");
      return t;
    }
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
        if (t->flags & TF_Any) return t;   /* narrowTypeByEquality: an any type is kept */
        TsNode *other = lm ? rt : l;
        TsType *ov = cExprType(other);
        if (ov == cErrorType) return cUnported("equality with an unported value");
        if (ov->flags & (TF_Undefined | TF_Null)) {
          bool holds = isEq == truthy;   /* the comparison holds on this branch */
          return cNullishEquality(t, ov, doubleEq, holds);
        }
        /* narrowTypeByEquality: the true branch keeps the members comparable with the value; the false branch of a unit
         * value removes the unit members; any other value leaves the type as it is */
        return cNarrowByEqualityValue(t, ov, doubleEq, isEq == truthy);
      }
      /* a discriminant: a property of the reference compared (getDiscriminantPropertyAccess: the left operand, then the right) */
      bool unp = false;
      TsNode *acc = cDiscriminantAccess(l, r, t, &unp);
      TsNode *accValue = rt;
      if (!acc && !unp) { acc = cDiscriminantAccess(rt, r, t, &unp); accValue = l; }
      if (unp) return cUnported("discriminant narrowing of an unported shape");
      if (acc) {
        if (acc->questionDotToken) return cUnported("discriminant narrowing of an optional chain");
        if (cHasKeyPropertyCandidate(t) && (op == TSK_EqualsEqualsEqualsToken || op == TSK_ExclamationEqualsEqualsToken))
          return cUnported("discriminant narrowing by a key property");
        CDiscEq e = { cExprType(accValue), doubleEq, isEq == truthy };
        TsType *res = cNarrowByDiscriminantWith(t, cDiscriminantName(acc), cDiscEqNarrow, &e);
        return res ? res : cUnported("discriminant narrowing of a union property");
      }
      return t;
    }
  }
  if (c->kind == TSK_CallExpression) {
    /* narrowTypeByCallExpression: a call that has the reference as an argument (or as the object of its callee) narrows by
     * the type predicate of its effects signature (an identifier predicate; a this predicate is not ported). A false
     * optional call has no effects signature. */
    if (!cHasMatchingArgument(c, r)) return t;
    if (!truthy && bIsOptionalChain(c)) return t;
    if (cExprType(c->expression) == cErrorType) return cUnported("narrowing by a call of an unported callee");
    TsNode *d = NULL;
    int k = cGetEffects(c, &d);
    if (k == CE_None) return t;
    if (k == CE_Unported) return cUnported("narrowing by a call");
    TsNode *p = d->type;
    if (p->kind != TSK_TypePredicate || p->assertsModifier) return t;
    if (!p->parameterName || p->parameterName->kind != TSK_Identifier) return cUnported("narrowing by a this predicate");
    return cNarrowByPredicate(c, truthy, t, r);
  }
  if (c->kind == TSK_PropertyAccessExpression || c->kind == TSK_ElementAccessExpression) {
    bool unp = false;
    TsNode *acc = cDiscriminantAccess(c, r, t, &unp);
    if (unp) return cUnported("discriminant truthiness of an unported shape");
    if (acc) {
      if (acc->questionDotToken) return cUnported("discriminant truthiness of an optional chain");
      CDiscTruthy d = { truthy };
      TsType *res = cNarrowByDiscriminantWith(t, cDiscriminantName(acc), cDiscTruthyNarrow, &d);
      return res ? res : cUnported("discriminant truthiness of a union property");
    }
  }
  /* narrowType: any other kind of condition leaves the type as it is (an identifier may be an aliased condition: not ported) */
  if (c->kind == TSK_Identifier) return cUnported("condition that mentions the reference");
  return t;
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

/* checkRightHandSideOfForOf, ported for an Array<T> and for a string (other iterables are not ported) */
static TsType *cForOfElementType(TsNode *stmt) {
  TsType *t = cExprType(stmt->expression);
  if (t == cErrorType) return t;
  if ((t->flags & TF_Object) && (t->objectFlags & OF_Reference) && t->target == cGlobalArrayDeclared() && t->nargs == 1)
    return t->args[0];
  if (t->flags & (TF_String | TF_StringLiteral)) return cStringType;
  return cUnported("for-of over an unported iterable");
}

/* the type assigned by an assignment flow node (getAssignedType, or getInitialType of a declaration): the initializer, the
 * right side of =, ||=, &&= or ??=, the element of a for-in (string) or for-of, and the binding element of a pattern. Compound
 * assignments are not here: cFlowAssignment takes them from the flow before. Destructuring defaults and elements are not ported. */
static TsType *cAssignedType(TsNode *n) {
  switch (n->kind) {
    case TSK_VariableDeclaration: {
      if (n->name && n->name->kind != TSK_Identifier) return cUnported("assignment of a pattern");
      if (n->initializer) return cExprType(n->initializer);
      TsNode *stmt = n->parent ? n->parent->parent : NULL;
      if (stmt && stmt->kind == TSK_ForInStatement) return cStringType;
      if (stmt && stmt->kind == TSK_ForOfStatement) return cForOfElementType(stmt);
      return cUnported("declaration without an initializer");
    }
    case TSK_BindingElement:
      if (!n->symbol) return cUnported("binding element without a symbol");
      return cBindingElementType(n->symbol, n);
    case TSK_Identifier: {
      TsNode *p = n->parent;
      switch (p->kind) {
        case TSK_BinaryExpression: {
          int op = p->operatorToken->kind;
          if (!(op == TSK_EqualsToken || op == TSK_BarBarEqualsToken || op == TSK_AmpersandAmpersandEqualsToken ||
                op == TSK_QuestionQuestionEqualsToken))
            return cUnported("compound assignment");
          if (p->parent->kind == TSK_ArrayLiteralExpression || p->parent->kind == TSK_PropertyAssignment)
            return cUnported("destructuring default assignment");
          return cExprType(p->right);
        }
        case TSK_ForInStatement: return cStringType;
        case TSK_ForOfStatement: return cForOfElementType(p);
        default: return cUnported("assignment of a destructuring element");
      }
    }
    default: return cUnported("assignment of another kind");
  }
}

/* ---- flow types (getTypeAtFlowNode, checker.ts 5.9.2 lines 74879-75165) ----
 * A flow type is a type with node's incomplete flag (an incomplete one is { flags: 0, type } in node). One walk of
 * getFlowTypeOfReference keeps node's shared flow memo (the Shared flow nodes it passed, by sharedFlowNodes and
 * sharedFlowTypes), the loop label caches (flowLoopCaches, by flow node and reference key), the loop stack of the labels in
 * progress (flowLoopNodes, flowLoopKeys, flowLoopTypes) and the reduce label overrides (target.antecedent). Node's
 * flowTypeCache of expression types is replaced by the rollback below: an expression type computed while a loop label is
 * in progress is dropped when that loop's back edge is done, since its flow types may be incomplete. */
typedef struct { TsType *t; bool inc; } CFT;
static CFT cFT(TsType *t) { CFT r = { t, false }; return r; }
static CFT cFlowAt(TsFlow *flow, CFlowRef *r);
/* createFlowType: an incomplete never is the silent never (node's silentNeverType, which is not the unreachable one) */
static CFT cCreateFlowType(TsType *t, bool inc) {
  CFT r = { (inc && (t->flags & TF_Never)) ? cNeverType : t, inc };
  return r;
}

/* unreachableNeverType: the never type of a flow that is unreachable (a call that never returns, an assignment in dead
 * code). It is not counted as a type, as it is not in the type count of the checker's intrinsics that are ported. */
static TsType cUnreachableNeverObj = { .flags = TF_Never };
#define cUnreachableNeverType (&cUnreachableNeverObj)

/* a growing list of types (the antecedentTypes of a label) */
typedef struct CTypeList { TsType **v; int n, cap; } CTypeList;
static void cTLAdd(CTypeList *l, TsType *t) {
  if (l->n == l->cap) {
    int cap = l->cap ? l->cap * 2 : 8;
    TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)cap);
    if (l->n) memcpy(v, l->v, sizeof(TsType *) * (size_t)l->n);
    l->v = v;
    l->cap = cap;
  }
  l->v[l->n++] = t;
}
static bool cTLContains(CTypeList *l, TsType *t) {
  for (int i = 0; i < l->n; i++) if (l->v[i] == t) return true;
  return false;
}
/* pushIfUnique */
static void cTLAddUnique(CTypeList *l, TsType *t) { if (!cTLContains(l, t)) cTLAdd(l, t); }

/* the loop label caches of node (flowLoopCaches): per flow node, the complete result of each reference key */
typedef struct CLoopCache { const char *key; TsType *t; struct CLoopCache *next; } CLoopCache;
/* the loop labels in progress (flowLoopNodes, flowLoopKeys, flowLoopTypes) and the expression log of their back edges */
typedef struct { TsFlow *flow; const char *key; CTypeList *types; int logMark; } CLoopEntry;
static CLoopEntry *cLoops;
static int cLoopN, cLoopCap;
static TsNode **cFlowLog;
static int cFlowLogN, cFlowLogCap;

/* the shared flow memo of the walks (sharedFlowNodes and sharedFlowTypes): one walk sees the entries from its start */
typedef struct { TsFlow *flow; CFT t; } CSharedFlow;
static CSharedFlow *cSharedFlows;
static int cSharedN, cSharedCap;

typedef struct { TsFlow *target; TsFlow **ants; int n; } CRedOverride;
static CRedOverride cRedOverrides[32];
static int cRedOverrideN;

/* the expression types computed while a loop back edge is walked (see cExprType) are logged, and dropped at the back edge's end */
static void cFlowLogAdd(TsNode *e) {
  if (cFlowLogN == cFlowLogCap) {
    cFlowLogCap = cFlowLogCap ? cFlowLogCap * 2 : 256;
    cFlowLog = realloc(cFlowLog, sizeof(TsNode *) * (size_t)cFlowLogCap);
    if (!cFlowLog) abort();
  }
  cFlowLog[cFlowLogN++] = e;
}

static void cLoopPush(TsFlow *flow, const char *key, CTypeList *types) {
  if (cLoopN == cLoopCap) {
    cLoopCap = cLoopCap ? cLoopCap * 2 : 64;
    cLoops = realloc(cLoops, sizeof(CLoopEntry) * (size_t)cLoopCap);
    if (!cLoops) abort();
  }
  cLoops[cLoopN].flow = flow;
  cLoops[cLoopN].key = key;
  cLoops[cLoopN].types = types;
  cLoops[cLoopN].logMark = cFlowLogN;
  cLoopN++;
}

static void cLoopPop(void) {
  CLoopEntry *top = &cLoops[--cLoopN];
  for (int i = top->logMark; i < cFlowLogN; i++) {
    cFlowLog[i]->exprState = 0;
    cFlowLog[i]->exprType = NULL;
  }
  cFlowLogN = top->logMark;
}

static TsType *cLoopCacheGet(TsFlow *f, const char *key) {
  for (CLoopCache *c = f->loopCache; c; c = c->next) if (strcmp(c->key, key) == 0) return c->t;
  return NULL;
}
static void cLoopCachePut(TsFlow *f, const char *key, TsType *t) {
  for (CLoopCache *c = f->loopCache; c; c = c->next) if (strcmp(c->key, key) == 0) { c->t = t; return; }
  CLoopCache *c = ts_arena_alloc(sizeof(CLoopCache));
  c->key = key;
  c->t = t;
  c->next = f->loopCache;
  f->loopCache = c;
}

/* the antecedents of a label, or the antecedents a reduce label gave its target */
static void cLabelAnts(TsFlow *f, TsFlow ***ants, int *n) {
  *ants = f->labelAnts;
  *n = f->nLabelAnts;
  for (int i = cRedOverrideN - 1; i >= 0; i--)
    if (cRedOverrides[i].target == f) { *ants = cRedOverrides[i].ants; *n = cRedOverrides[i].n; return; }
}

/* isTypeSubsetOf (the enum-like case of isTypeSubsetOfUnion is not ported) */
static bool cIsTypeSubsetOf(TsType *s, TsType *t) {
  if (s == t || (s->flags & TF_Never)) return true;
  if (!(t->flags & TF_Union)) return false;
  if (s->flags & TF_Union) {
    for (int i = 0; i < s->ntypes; i++) {
      bool in = false;
      for (int j = 0; j < t->ntypes && !in; j++) in = t->types[j] == s->types[i];
      if (!in) return false;
    }
    return true;
  }
  for (int j = 0; j < t->ntypes; j++) if (t->types[j] == s) return true;
  return false;
}

/* getUnionOrEvolvingArrayType (the evolving arrays are not ported): the union of the types with the literal or the subtype
 * reduction, and the declared type when the union has the members of the declared union */
static TsType *cFlowUnion(TsType **ts, int n, bool subtypeRed, TsType *declared) {
  TsType *res = subtypeRed ? cUnionWithSubtypesRemoved(ts, n) : cGetUnionType(ts, n);
  if (res != declared && (res->flags & TF_Union) && (declared->flags & TF_Union) && res->ntypes == declared->ntypes &&
      memcmp(res->types, declared->types, sizeof(TsType *) * (size_t)res->ntypes) == 0)
    return declared;
  return res;
}

/* the reference key of a flow cache (getFlowCacheKey): the symbol of the root identifier and the property names of the chain */
static char *cFlowCat(const char *a, const char *sep, const char *b, int blen) {
  size_t la = strlen(a), ls = strlen(sep);
  char *s = ts_arena_alloc(la + ls + (size_t)blen + 1);
  memcpy(s, a, la);
  memcpy(s + la, sep, ls);
  memcpy(s + la + ls, b, (size_t)blen);
  s[la + ls + (size_t)blen] = 0;
  return s;
}
static const char *cFlowChainKey(TsNode *n) {
  for (;;) {
    if (n->kind == TSK_ParenthesizedExpression || n->kind == TSK_NonNullExpression) { n = n->expression; continue; }
    break;
  }
  if (n->kind == TSK_Identifier) {
    TsSymbol *s = cResolveValueName(n, bnEscapeText(n->text, n->textLen));
    if (!s) return NULL;
    char *b = ts_arena_alloc(32);
    snprintf(b, 32, "@%p", (void *)s);
    return b;
  }
  if (n->kind == TSK_PropertyAccessExpression && n->name) {
    const char *k = cFlowChainKey(n->expression);
    return k ? cFlowCat(k, ".", n->name->text, n->name->textLen) : NULL;
  }
  if (n->kind == TSK_ElementAccessExpression) {
    TsNode *a = cSkipParens(n->argumentExpression);
    if (!a || (a->kind != TSK_StringLiteral && a->kind != TSK_NumericLiteral)) return NULL;
    const char *k = cFlowChainKey(n->expression);
    return k ? cFlowCat(k, ".", a->text, a->textLen) : NULL;
  }
  return NULL;
}
/* getFlowCacheKey of a reference: the container, the declared and the initial type, and the chain */
static const char *cFlowRefKey(CFlowRef *r) {
  if (!r->keyDone) {
    r->keyDone = true;
    const char *chain = cFlowChainKey(r->node);
    if (chain) {
      size_t cap = strlen(chain) + 96;
      char *b = ts_arena_alloc(cap);
      snprintf(b, cap, "%p|%p|%p%s", (void *)r->flowContainer, (void *)r->declared, (void *)r->initial, chain);
      r->key = b;
    }
  }
  return r->key;
}

/* isFalseExpression */
static bool cIsFalseExpr(TsNode *e) {
  TsNode *n = cSkipParens(e);
  if (n->kind == TSK_FalseKeyword) return true;
  if (n->kind == TSK_BinaryExpression) {
    int op = n->operatorToken->kind;
    if (op == TSK_AmpersandAmpersandToken) return cIsFalseExpr(n->left) || cIsFalseExpr(n->right);
    if (op == TSK_BarBarToken) return cIsFalseExpr(n->left) && cIsFalseExpr(n->right);
  }
  return false;
}

/* getTypePredicateOfSignature and hasTypePredicateOrNeverReturnType on a declaration; the return type is the annotation */
static bool cIsNeverReturn(TsNode *d) {
  return d && d->type && d->type->kind != TSK_TypePredicate && (cTypeFromNode(d->type)->flags & TF_Never) != 0;
}
static bool cSigHasPredOrNever(TsNode *d) {
  return d && d->type && (d->type->kind == TSK_TypePredicate || cIsNeverReturn(d));
}

/* the parameter an asserts predicate names (its parameterIndex), or -1 */
static int cAssertParamIndex(TsNode *d, TsNode *pred) {
  CParams ps;
  if (!cSplitParams(d, &ps)) return -1;
  int idx = -1;
  for (int k = 0; k < ps.n; k++) {
    TsNode *pn = ps.params[k]->name;
    if (pn && pn->kind == TSK_Identifier && pn->textLen == pred->parameterName->textLen &&
        memcmp(pn->text, pred->parameterName->text, (size_t)pn->textLen) == 0) idx = k;
  }
  return idx;
}

/* the effects signature of a call (getEffectsSignature): none, the single declared signature, or a case not ported (several
 * signatures with a predicate or a never return, a generic one, a union or other callee type). The callee is the expression
 * type (node uses the dotted name in an expression statement). An error or any callee has no effect. */
static int cGetEffects(TsNode *call, TsNode **decl) {
  *decl = NULL;
  if (call->kind != TSK_CallExpression || call->expression->kind == TSK_SuperKeyword) return CE_None;
  TsType *f = cExprType(call->expression);
  if (f == cErrorType || (f->flags & (TF_Any | TF_Unknown))) return CE_None;
  if (!(f->flags & TF_Object)) {
    if (f->flags & (TF_Union | TF_Intersection | TF_TypeParameter | TF_Index | TF_IndexedAccess)) return CE_Unported;
    return CE_None;
  }
  cResolveSignatures(f);
  bool eff = false;
  for (int i = 0; i < f->ncall && !eff; i++) eff = cSigHasPredOrNever(f->call[i].decl);
  if (!eff) return CE_None;
  if (f->ncall == 1 && f->call[0].decl && !(f->call[0].decl->typeParameters && f->call[0].decl->typeParameters->n)) {
    *decl = f->call[0].decl;
    return CE_Sig;
  }
  return CE_Unported;
}

static bool cIsExhaustiveSwitch(TsNode *sw);

/* isReachableFlowNode (isReachableFlowNodeWorker): a flow node is reachable unless a path back from it is unreachable (the
 * start of the walk is not reached through an unreachable node, a never-returning call or a false assertion, or an exhaustive
 * switch with no clause). The result of a shared node is kept in the node. */
static bool cIsReachableFlow(TsFlow *flow, bool noCacheCheck) {
  for (;;) {
    uint32_t fl = flow->flags;
    if (fl & FF_Shared) {
      if (!noCacheCheck) {
        if (flow->reach) return flow->reach == 1;
        bool v = cIsReachableFlow(flow, true);
        flow->reach = v ? 1 : 2;
        return v;
      }
      noCacheCheck = false;
    }
    if (fl & (FF_Assignment | FF_TrueCondition | FF_FalseCondition | FF_ArrayMutation)) { flow = flow->ante; continue; }
    if (fl & FF_Call) {
      TsNode *d = NULL;
      int k = cGetEffects(flow->node, &d);
      if (k == CE_Unported) { (void)cUnported("reachability of an overloaded or generic callee"); return true; }
      if (k == CE_Sig) {
        TsNode *p = d->type;
        if (p->kind == TSK_TypePredicate && p->assertsModifier && p->parameterName && !p->type) {
          int idx = cAssertParamIndex(d, p);
          if (idx >= 0 && flow->node->arguments && idx < flow->node->arguments->n && cIsFalseExpr(flow->node->arguments->v[idx]))
            return false;
        }
        if (cIsNeverReturn(d)) return false;
      }
      flow = flow->ante;
      continue;
    }
    if (fl & FF_BranchLabel) {
      TsFlow **ants; int n;
      cLabelAnts(flow, &ants, &n);
      for (int i = 0; i < n; i++) if (cIsReachableFlow(ants[i], false)) return true;
      return false;
    }
    if (fl & FF_LoopLabel) {
      TsFlow **ants; int n;
      cLabelAnts(flow, &ants, &n);
      if (n == 0) return false;
      flow = ants[0];
      continue;
    }
    if (fl & FF_SwitchClause) {
      if (flow->clauseStart == flow->clauseEnd && cIsExhaustiveSwitch(flow->node)) return false;
      flow = flow->ante;
      continue;
    }
    if (fl & FF_ReduceLabel) {
      if (cRedOverrideN == 32) { (void)cUnported("reduce label depth"); return true; }
      cRedOverrides[cRedOverrideN].target = flow->target;
      cRedOverrides[cRedOverrideN].ants = flow->redAnts;
      cRedOverrides[cRedOverrideN].n = flow->nRedAnts;
      cRedOverrideN++;
      bool res = cIsReachableFlow(flow->ante, false);
      cRedOverrideN--;
      return res;
    }
    return !(fl & FF_Unreachable);
  }
}

/* ---- switch clauses (checker.ts 5.9.2: getSwitchClauseTypes, narrowTypeBySwitchOn*) ---- */
/* getSwitchClauseTypes: the regular type of each case expression (never for the default clause) */
static TsType **cSwitchClauseTypes(TsNode *sw) {
  TsList *cl = sw->caseBlock->clauses;
  int n = cl ? cl->n : 0;
  TsType **st = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  for (int i = 0; i < n; i++) {
    TsNode *c = cl->v[i];
    st[i] = c->kind == TSK_CaseClause ? cRegularType(cExprType(c->expression)) : cNeverType;
  }
  return st;
}
/* the index of the first default clause, or -1 */
static int cSwitchDefaultIndex(TsNode *sw) {
  TsList *cl = sw->caseBlock->clauses;
  for (int i = 0; cl && i < cl->n; i++) if (cl->v[i]->kind == TSK_DefaultClause) return i;
  return -1;
}
/* getSwitchClauseTypeOfWitnesses: the text of each case (its first occurrence, not empty), or NULL when a case is not a string
 * literal. The witnesses are the case nodes; NULL entries have no witness. */
static TsNode **cSwitchTypeofWitnesses(TsNode *sw, int *nOut) {
  TsList *cl = sw->caseBlock->clauses;
  int n = cl ? cl->n : 0;
  *nOut = n;
  for (int i = 0; i < n; i++) {
    TsNode *c = cl->v[i];
    if (c->kind == TSK_CaseClause && !bIsStringLiteralLike(c->expression)) return NULL;
  }
  TsNode **w = ts_arena_alloc(sizeof(TsNode *) * (size_t)(n ? n : 1));
  for (int i = 0; i < n; i++) {
    TsNode *c = cl->v[i];
    w[i] = NULL;
    if (c->kind != TSK_CaseClause || c->expression->textLen == 0) continue;
    bool dup = false;
    for (int j = 0; j < i && !dup; j++)
      dup = w[j] && w[j]->textLen == c->expression->textLen && memcmp(w[j]->text, c->expression->text, (size_t)w[j]->textLen) == 0;
    if (!dup) w[i] = c->expression;
  }
  return w;
}
/* getNotEqualFactsFromTypeofSwitch: the not-equal facts of the witnesses outside [start, end) */
static uint32_t cNotEqualFactsFromTypeof(int start, int end, TsNode **w, int n) {
  uint32_t f = 0;
  for (int i = 0; i < n; i++)
    if ((i < start || i >= end) && w[i]) f |= cTypeofNeFacts(w[i]->text, w[i]->textLen);
  return f;
}
/* filterType (t => getTypeFacts(t, mask) === mask) */
static TsType *cFilterFactsEq(TsType *t, uint32_t mask) {
  if (t == cErrorType) return t;
  int n = cMemberCount(t);
  TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  int k = 0;
  bool changed = false;
  for (int i = 0; i < n; i++) {
    TsType *m = cMemberAt(t, i);
    bool ok = true;
    uint32_t mf = cTypeFacts(m, &ok);
    if (!ok) return cUnported("typeof switch of a type that is not ported");
    if ((mf & mask) == mask) kept[k++] = m; else changed = true;
  }
  if (!changed) return t;
  if (k == 0) return cNeverType;
  return k == 1 ? kept[0] : cGetUnionType(kept, k);
}
/* narrowTypeBySwitchOnDiscriminant: the members comparable with the case types, and for a default clause the members that
 * are not one of the case types */
static TsType *cSwitchOnDiscriminant(TsType *t, TsNode *sw, int start, int end) {
  if (t == cErrorType) return t;
  TsType **st = cSwitchClauseTypes(sw);
  int nAll = sw->caseBlock->clauses ? sw->caseBlock->clauses->n : 0;
  if (nAll == 0) return t;
  TsType **ct = st + start;
  int cn = end - start;
  bool hasDefault = start == end;
  for (int i = 0; i < cn; i++) if (ct[i] == cNeverType) hasDefault = true;
  if ((t->flags & TF_Unknown) && !hasDefault) return cUnported("switch narrowing of unknown without a default");
  TsType *disc = cGetUnionType(ct, cn);
  TsType *caseType;
  if (disc->flags & TF_Never) caseType = cNeverType;
  else {
    int n = cMemberCount(t);
    TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
    int k = 0;
    for (int i = 0; i < n; i++) if (cAreTypesComparable(disc, cMemberAt(t, i))) kept[k++] = cMemberAt(t, i);
    TsType *filtered = k == 0 ? cNeverType : (k == 1 ? kept[0] : cGetUnionType(kept, k));
    caseType = cReplacePrimitivesWithLiterals(filtered, disc);
  }
  if (!hasDefault) return caseType;
  int n = cMemberCount(t);
  TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  int k = 0;
  for (int i = 0; i < n; i++) {
    TsType *m = cMemberAt(t, i);
    bool inSwitch = false;
    if (cIsUnitType(m)) {
      TsType *rm = cRegularType(m);
      for (int j = 0; j < nAll && !inSwitch; j++) inSwitch = st[j] == rm;
    }
    if (!inSwitch) kept[k++] = m;
  }
  TsType *defType = k == 0 ? cNeverType : (k == 1 ? kept[0] : cGetUnionType(kept, k));
  if (caseType->flags & TF_Never) return defType;
  TsType *pair[2] = { caseType, defType };
  return cGetUnionType(pair, 2);
}

/* narrowTypeBySwitchOnTypeOf: the typeof names of the clauses (a default clause keeps the operand's types that are not
 * excluded by the other names) */
static TsType *cSwitchOnTypeOf(TsType *t, TsNode *sw, int start, int end) {
  if (t == cErrorType) return t;
  int n;
  TsNode **w = cSwitchTypeofWitnesses(sw, &n);
  if (!w) return t;
  int defIdx = cSwitchDefaultIndex(sw);
  bool hasDefault = start == end || (defIdx >= start && defIdx < end);
  if (hasDefault) return cFilterFactsEq(t, cNotEqualFactsFromTypeof(start, end, w, n));
  TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(end - start > 0 ? end - start : 1));
  for (int i = start; i < end; i++)
    parts[i - start] = w[i] ? cNarrowByTypeName(t, w[i]->text, w[i]->textLen) : cNeverType;
  return cGetUnionType(parts, end - start);
}

/* narrowTypeBySwitchOnTrue: each case expression narrows the reference as a condition that is false before the clauses of
 * the flow, and as true for the clauses of the flow (or as false for the clauses after them when there is a default) */
static TsType *cSwitchOnTrue(TsType *t, TsNode *sw, int start, int end, CFlowRef *r) {
  TsList *cl = sw->caseBlock->clauses;
  int n = cl ? cl->n : 0;
  int defIdx = cSwitchDefaultIndex(sw);
  bool hasDefault = start == end || (defIdx >= start && defIdx < end);
  for (int i = 0; i < start; i++)
    if (cl->v[i]->kind == TSK_CaseClause) t = cNarrowByCondition(cl->v[i]->expression, false, t, r);
  if (hasDefault) {
    for (int i = end; i < n; i++)
      if (cl->v[i]->kind == TSK_CaseClause) t = cNarrowByCondition(cl->v[i]->expression, false, t, r);
    return t;
  }
  TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(end - start));
  for (int i = start; i < end; i++) {
    TsNode *c = cl->v[i];
    parts[i - start] = c->kind == TSK_CaseClause ? cNarrowByCondition(c->expression, true, t, r) : cNeverType;
  }
  return cGetUnionType(parts, end - start);
}

/* isExhaustiveSwitchStatement for the switches whose operand is a literal type (computeExhaustiveSwitchStatement); a switch on
 * typeof is not ported and is not exhaustive here */
static bool cIsLiteralTypeFlow(TsType *t) {
  if (t->flags & TF_Boolean) return true;
  if (t->flags & TF_Union) {
    if (t->flags & TF_EnumLiteral) return true;
    for (int i = 0; i < t->ntypes; i++) if (!cIsUnitType(t->types[i])) return false;
    return true;
  }
  return cIsUnitType(t);
}
static bool cIsExhaustiveSwitch(TsNode *sw) {
  TsNode *e = sw->expression;
  if (e->kind == TSK_TypeOfExpression) {
    /* computeExhaustiveSwitchStatement for typeof: every typeof name that the operand may have is a case */
    int n;
    TsNode **w = cSwitchTypeofWitnesses(sw, &n);
    if (!w) return false;
    TsType *operand = cExprType(e->expression);
    if (operand == cErrorType) return false;
    TsType *oc = cBaseConstraintOf(operand);
    if (!oc) oc = operand;
    uint32_t nef = cNotEqualFactsFromTypeof(0, 0, w, n);
    if (oc->flags & (TF_Any | TF_Unknown)) return (556800u & nef) == 556800u;   /* AllTypeofNE */
    int m = cMemberCount(oc);
    for (int i = 0; i < m; i++) {
      bool ok = true;
      uint32_t mf = cTypeFacts(cMemberAt(oc, i), &ok);
      if (!ok) { (void)cUnported("exhaustive typeof switch of a type that is not ported"); return false; }
      if ((mf & nef) == nef) return false;
    }
    return true;
  }
  TsType *t = cExprType(e);
  if (t == cErrorType) return false;
  TsType *bc = cBaseConstraintOf(t);
  if (!bc) bc = t;
  if (bc == cErrorType || !cIsLiteralTypeFlow(bc)) return false;
  TsList *cl = sw->caseBlock->clauses;
  int n = cl ? cl->n : 0;
  if (n == 0) return false;
  TsType **st = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
  for (int i = 0; i < n; i++) {
    TsNode *c = cl->v[i];
    st[i] = c->kind == TSK_CaseClause ? cRegularType(cExprType(c->expression)) : cNeverType;
  }
  for (int i = 0; i < n; i++) if (!(cIsUnitType(st[i]) || (st[i]->flags & TF_Never))) return false;
  /* eachTypeContainedIn (the members of the regular literal type, each in the switch types) */
  int m = (bc->flags & TF_Union) ? bc->ntypes : 1;
  for (int j = 0; j < m; j++) {
    TsType *mt = cRegularType((bc->flags & TF_Union) ? bc->types[j] : bc);
    bool in = false;
    for (int i = 0; i < n && !in; i++) in = st[i] == mt;
    if (!in) return false;
  }
  return true;
}

/* narrowTypeByAssertion (75001): the argument of an assertion function narrowed as a condition that holds. `a && b` narrows
 * by a and then by b, `a || b` is the union of both, and `false` makes the flow unreachable. */
static TsType *cNarrowByAssertionExpr(TsNode *e, TsType *t, CFlowRef *r) {
  TsNode *n = cSkipParens(e);
  if (n->kind == TSK_FalseKeyword) return cUnreachableNeverType;
  if (n->kind == TSK_BinaryExpression && n->operatorToken->kind == TSK_AmpersandAmpersandToken)
    return cNarrowByAssertionExpr(n->right, cNarrowByAssertionExpr(n->left, t, r), r);
  if (n->kind == TSK_BinaryExpression && n->operatorToken->kind == TSK_BarBarToken) {
    TsType *a = cNarrowByAssertionExpr(n->left, t, r);
    TsType *b = cNarrowByAssertionExpr(n->right, t, r);
    if (a == cErrorType || b == cErrorType) return cErrorType;
    TsType *pair[2] = { a, b };
    return cGetUnionType(pair, 2);
  }
  return cNarrowByCondition(n, true, t, r);
}

/* getTypeAtFlowCall's assertion narrowing (5.9.2): the reference narrowed by the predicate of an assertion call. `asserts x is
 * T` narrows by the predicate type, `asserts x` by the argument's condition. Assertions of this and of overloaded callees
 * are not ported. */
static TsType *cAssertionCallNarrowing(TsNode *call, TsNode *d, TsType *t, CFlowRef *r) {
  TsNode *pred = d->type;
  if (!pred || pred->kind != TSK_TypePredicate || !pred->assertsModifier) return t;   /* a plain predicate has no flow effect */
  if (!pred->parameterName || pred->parameterName->kind != TSK_Identifier) return cUnported("assertion of this");
  CParams ps;
  if (!cSplitParams(d, &ps) || ps.thisParam || ps.rest) return cUnported("assertion call of a rest or this parameter");
  int idx = cAssertParamIndex(d, pred);
  if (idx < 0 || !call->arguments || idx >= call->arguments->n) return cUnported("assertion call without its argument");
  TsNode *arg = call->arguments->v[idx];
  if (!pred->type) return cNarrowByAssertionExpr(arg, t, r);
  if (!cMatchesRefTree(cSkipParens(arg), r)) return cMentionsText(arg, r->text, r->len) ? cUnported("assertion of another reference") : t;
  TsType *cand = cTypeFromNode(pred->type);
  if (cand == cErrorType || (cand->flags & (TF_Any | TF_Unknown))) return cUnported("assertion to any or unknown");
  if (t == cErrorType) return t;
  return cGetNarrowedType(t, cand, true);
}

static bool cArgsMention(TsNode *call, CFlowRef *r) {
  if (!call->arguments) return false;
  for (int i = 0; i < call->arguments->n; i++) if (cMentionsText(call->arguments->v[i], r->text, r->len)) return true;
  return false;
}

enum { AK_None = 0, AK_Definite = 1, AK_Compound = 2 };
static TsNode *cAssignmentTarget(TsNode *node);
static int cAssignmentTargetKind(TsNode *id);
static TsType *cBaseOfLiteral(TsType *t, bool *unp);
static bool cIsInCompoundLikeAssignment(TsNode *id);
/* getTypeAtFlowAssignment: the reference assigned here (its declared type, or the assigned type reduced by it), the
 * reference's container assigned (the declared type), or no effect (false). An assignment in unreachable code is never. */
static bool cFlowAssignment(TsFlow *flow, CFlowRef *r, CFT *out) {
  TsNode *tg = cAssignTarget(flow->node);
  TsType *declared = r->declared;
  if (tg && cAssignMatches(tg, r)) {
    if (!cIsReachableFlow(flow, false)) { *out = cFT(cUnreachableNeverType); return true; }
    if (cAssignmentTargetKind(flow->node) == AK_Compound) {
      /* a compound assignment (x += v, x++) is the base literal type of the flow before it */
      CFT prev = cFlowAt(flow->ante, r);
      bool unp = false;
      TsType *base = cBaseOfLiteral(prev.t, &unp);
      *out = unp ? cFT(cUnported("compound assignment of a bigint or enum literal")) : cCreateFlowType(base, prev.inc);
      return true;
    }
    bool unp = false;
    TsType *t = cIsInCompoundLikeAssignment(flow->node) ? cBaseOfLiteral(declared, &unp) : declared;
    if (unp) { *out = cFT(cUnported("assignment to a bigint or enum literal type")); return true; }
    if (!(t->flags & TF_Union)) { *out = cFT(t); return true; }
    TsType *assigned = cAssignedType(flow->node);
    *out = cFT(assigned == cErrorType ? assigned : cAssignmentReduced(t, assigned));
    return true;
  }
  /* containsMatchingReference (x.y = v for the reference x): the declared type, whatever it is (getTypeAtFlowAssignment) */
  if (tg && cAssignContains(tg, r)) {
    if (!cIsReachableFlow(flow, false)) { *out = cFT(cUnreachableNeverType); return true; }
    *out = cFT(declared);
    return true;
  }
  if (tg && tg->kind != TSK_Identifier && tg->kind != TSK_PropertyAccessExpression && tg->kind != TSK_ElementAccessExpression &&
      cMentionsText(tg, r->text, r->len)) {
    *out = cFT(cUnported("destructuring assignment of the reference"));
    return true;
  }
  return false;
}

/* getTypeAtFlowCall: an assertion call narrows the reference, a call that never returns makes the flow unreachable, and
 * any other call has no effect (false) */
static bool cFlowCall(TsFlow *flow, CFlowRef *r, CFT *out) {
  TsNode *call = flow->node;
  TsNode *d = NULL;
  int k = cGetEffects(call, &d);
  if (k == CE_None) return false;
  if (k == CE_Unported) { *out = cFT(cUnported("effects of an overloaded or generic callee")); return true; }
  TsNode *pred = d->type;
  if (pred->kind == TSK_TypePredicate && pred->assertsModifier) {
    CFT ft = cFlowAt(flow->ante, r);
    if (!cArgsMention(call, r)) { *out = ft; return true; }
    TsType *n = cAssertionCallNarrowing(call, d, ft.t, r);
    *out = n == ft.t ? ft : cCreateFlowType(n, ft.inc);
    return true;
  }
  if (cIsNeverReturn(d)) { *out = cFT(cUnreachableNeverType); return true; }
  return false;
}

/* getTypeAtFlowCondition: the antecedent narrowed by the condition (a never antecedent stays as it is) */
static CFT cFlowCondition(TsFlow *flow, CFlowRef *r) {
  CFT ft = cFlowAt(flow->ante, r);
  TsType *t = ft.t;
  if (t == cErrorType || (t->flags & TF_Never)) return ft;
  bool truthy = (flow->flags & FF_TrueCondition) != 0;
  TsType *n = cNarrowByCondition(flow->node, truthy, t, r);
  if (n == t) return ft;
  return cCreateFlowType(n, ft.inc);
}

/* getTypeAtSwitchClause: the antecedent, narrowed by the switch when the switch operand is the reference (its discriminant), the
 * typeof of the reference, or `true`. A switch on a property of the reference (a discriminant property) is not ported. */
static CFT cFlowSwitchClause(TsFlow *flow, CFlowRef *r) {
  CFT ft = cFlowAt(flow->ante, r);
  TsType *t = ft.t;
  if (t == cErrorType) return ft;
  TsNode *sw = flow->node;
  TsNode *expr = cSkipParens(sw->expression);
  int start = flow->clauseStart, end = flow->clauseEnd;
  TsType *n = t;
  if (cMatchesRefTree(expr, r)) n = cSwitchOnDiscriminant(t, sw, start, end);
  else if (expr->kind == TSK_TypeOfExpression && cMatchesRefTree(cSkipParens(expr->expression), r)) n = cSwitchOnTypeOf(t, sw, start, end);
  else if (expr->kind == TSK_TrueKeyword) n = cSwitchOnTrue(t, sw, start, end, r);
  else if (cMentionsText(sw->expression, r->text, r->len)) return cFT(cUnported("switch narrowing of a discriminant property"));
  if (n == t) return ft;
  return cCreateFlowType(n, ft.inc);
}

/* getTypeAtFlowBranchLabel: the union of the antecedent types (subtype reduction when one is not a subset of the initial type).
 * The implicit default of a switch without a default clause is a bypass, unless the switch is exhaustive. */
static CFT cFlowBranchLabel(TsFlow *flow, CFlowRef *r) {
  TsType *declared = r->declared, *initial = r->initial;
  TsFlow **ants; int n;
  cLabelAnts(flow, &ants, &n);
  CTypeList *list = ts_arena_zalloc(sizeof(CTypeList));
  bool subtypeRed = false, seenInc = false;
  TsFlow *bypass = NULL;
  for (int i = 0; i < n; i++) {
    TsFlow *a = ants[i];
    if (!bypass && (a->flags & FF_SwitchClause) && a->clauseStart == a->clauseEnd) { bypass = a; continue; }
    CFT ft = cFlowAt(a, r);
    TsType *t = ft.t;
    if (t == declared && declared == initial) return cFT(t);
    cTLAddUnique(list, t);
    if (!cIsTypeSubsetOf(t, initial)) subtypeRed = true;
    if (ft.inc) seenInc = true;
  }
  if (bypass) {
    CFT ft = cFlowAt(bypass, r);
    TsType *t = ft.t;
    if (!(t->flags & TF_Never) && !cTLContains(list, t) && !cIsExhaustiveSwitch(bypass->node)) {
      if (t == declared && declared == initial) return cFT(t);
      cTLAdd(list, t);
      if (!cIsTypeSubsetOf(t, initial)) subtypeRed = true;
      if (ft.inc) seenInc = true;
    }
  }
  return cCreateFlowType(cFlowUnion(list->v, list->n, subtypeRed, declared), seenInc);
}

/* getTypeAtFlowLoopLabel: the cached type of the reference at the loop, else the union of its antecedents. The first antecedent
 * is walked outside the loop; each other one is walked with the loop in progress, so a back edge that reaches the loop again
 * gets the union so far, incomplete. A complete result is cached. */
static CFT cFlowLoopLabel(TsFlow *flow, CFlowRef *r) {
  TsType *declared = r->declared;
  const char *key = cFlowRefKey(r);
  if (!key) return cFT(declared);
  TsType *cached = cLoopCacheGet(flow, key);
  if (cached) return cFT(cached);
  for (int i = 0; i < cLoopN; i++) {
    if (cLoops[i].flow == flow && strcmp(cLoops[i].key, key) == 0 && cLoops[i].types->n)
      return cCreateFlowType(cFlowUnion(cLoops[i].types->v, cLoops[i].types->n, false, declared), true);
  }
  TsFlow **ants; int n;
  cLabelAnts(flow, &ants, &n);
  if (n == 0) return cFT(cNeverType);
  CTypeList *list = ts_arena_zalloc(sizeof(CTypeList));
  bool subtypeRed = false;
  CFT first = { NULL, false };
  for (int i = 0; i < n; i++) {
    CFT ft;
    if (i == 0) {
      first = cFlowAt(ants[0], r);
      ft = first;
    } else {
      cLoopPush(flow, key, list);
      ft = cFlowAt(ants[i], r);
      cLoopPop();
      TsType *c2 = cLoopCacheGet(flow, key);
      if (c2) return cFT(c2);
    }
    TsType *t = ft.t;
    cTLAddUnique(list, t);
    if (!cIsTypeSubsetOf(t, r->initial)) subtypeRed = true;
    if (t == declared) break;
  }
  TsType *res = cFlowUnion(list->v, list->n, subtypeRed, declared);
  if (first.inc) return cCreateFlowType(res, true);
  cLoopCachePut(flow, key, res);
  return cFT(res);
}

/* getTypeAtFlowNode: the flow type of the reference at a flow node. The walk follows the antecedents of the assignments,
 * calls, conditions and switch clauses that do not apply to it; a label joins its antecedents; the start of the container
 * gives the initial type (or continues at the function value's creation flow). A Shared node is memoized for this walk. */
static CFT cFlowAt(TsFlow *flow, CFlowRef *r) {
  if (r->depth >= 2000) return cFT(cUnported("flow depth"));
  r->depth++;
  TsFlow *sharedFlow = NULL;
  CFT type;
  for (;;) {
    uint32_t fl = flow->flags;
    if (fl & FF_Shared) {
      for (int i = r->sharedStart; i < cSharedN; i++)
        if (cSharedFlows[i].flow == flow) { r->depth--; return cSharedFlows[i].t; }
      sharedFlow = flow;
    }
    if (fl & FF_Assignment) {
      if (!cFlowAssignment(flow, r, &type)) { flow = flow->ante; continue; }
    } else if (fl & FF_Call) {
      if (!cFlowCall(flow, r, &type)) { flow = flow->ante; continue; }
    } else if (fl & (FF_TrueCondition | FF_FalseCondition)) {
      type = cFlowCondition(flow, r);
    } else if (fl & FF_SwitchClause) {
      type = cFlowSwitchClause(flow, r);
    } else if (fl & (FF_BranchLabel | FF_LoopLabel)) {
      TsFlow **ants; int n;
      cLabelAnts(flow, &ants, &n);
      if (n == 1) { flow = ants[0]; continue; }
      type = (fl & FF_BranchLabel) ? cFlowBranchLabel(flow, r) : cFlowLoopLabel(flow, r);
    } else if (fl & FF_ArrayMutation) {
      /* getTypeAtFlowArrayMutation: only the evolving arrays of an auto type, which are not ported */
      flow = flow->ante;
      continue;
    } else if (fl & FF_ReduceLabel) {
      if (cRedOverrideN == 32) {
        type = cFT(cUnported("reduce label depth"));
      } else {
        cRedOverrides[cRedOverrideN].target = flow->target;
        cRedOverrides[cRedOverrideN].ants = flow->redAnts;
        cRedOverrides[cRedOverrideN].n = flow->nRedAnts;
        cRedOverrideN++;
        type = cFlowAt(flow->ante, r);
        cRedOverrideN--;
      }
    } else if (fl & FF_Start) {
      /* FlowStart: a function value created in another container continues at its creation flow */
      TsNode *c = flow->node;
      if (r->jump && c && c != r->flowContainer) {
        if (!c->flowNode) { type = cFT(cUnported("function value without a flow node")); }
        else { flow = c->flowNode; continue; }
      } else {
        type = cFT(cFlowStartType(r));
      }
    } else {
      /* an unreachable flow node: the declared type (convertAutoToAny) */
      type = cFT(r->declared);
    }
    if (sharedFlow) {
      if (cSharedN == cSharedCap) {
        cSharedCap = cSharedCap ? cSharedCap * 2 : 256;
        cSharedFlows = realloc(cSharedFlows, sizeof(CSharedFlow) * (size_t)cSharedCap);
        if (!cSharedFlows) abort();
      }
      cSharedFlows[cSharedN].flow = sharedFlow;
      cSharedFlows[cSharedN].t = type;
      cSharedN++;
    }
    r->depth--;
    return type;
  }
}

/* ---- assignment targets and the last assignment of a variable (checker.ts 5.9.2) ---- */
#define CMAX_ASSIGN_POS INT64_MAX / 4

/* getAssignmentTarget: the assignment, update or for-in/of statement that writes the identifier, else NULL */
static TsNode *cAssignmentTarget(TsNode *node) {
  TsNode *parent = node->parent;
  for (;;) {
    switch (parent->kind) {
      case TSK_BinaryExpression:
        return isAssignmentOperator(parent->operatorToken->kind) && parent->left == node ? parent : NULL;
      case TSK_PrefixUnaryExpression: case TSK_PostfixUnaryExpression:
        return (parent->extra == TSK_PlusPlusToken || parent->extra == TSK_MinusMinusToken) ? parent : NULL;
      case TSK_ForInStatement: case TSK_ForOfStatement:
        return parent->initializer == node ? parent : NULL;
      case TSK_ParenthesizedExpression: case TSK_ArrayLiteralExpression: case TSK_SpreadElement: case TSK_NonNullExpression:
        node = parent;
        break;
      case TSK_SpreadAssignment:
        node = parent->parent;
        break;
      case TSK_ShorthandPropertyAssignment:
        if (parent->name != node) return NULL;
        node = parent->parent;
        break;
      case TSK_PropertyAssignment:
        if (parent->name == node) return NULL;
        node = parent->parent;
        break;
      default:
        return NULL;
    }
    parent = node->parent;
  }
}

/* isInCompoundLikeAssignment: the identifier is the target of `x = a + b` (a shift, additive, multiplicative or exponent
 * operator on the right), whose declared type is widened to its base literal types */
static bool cIsInCompoundLikeAssignment(TsNode *id) {
  TsNode *t = cAssignmentTarget(id);
  if (!t || t->kind != TSK_BinaryExpression || t->operatorToken->kind != TSK_EqualsToken) return false;
  TsNode *right = cSkipParens(t->right);
  return right->kind == TSK_BinaryExpression && getBinaryOperatorPrecedence(right->operatorToken->kind) >= OP_Shift;
}

/* getAssignmentTargetKind: definite for `=`, the logical assignments, and for-in/of; compound for the other operators and ++ or -- */
static int cAssignmentTargetKind(TsNode *id) {
  TsNode *t = cAssignmentTarget(id);
  if (!t) return AK_None;
  switch (t->kind) {
    case TSK_BinaryExpression: {
      int op = t->operatorToken->kind;
      return (op == TSK_EqualsToken || op == TSK_BarBarEqualsToken || op == TSK_AmpersandAmpersandEqualsToken ||
              op == TSK_QuestionQuestionEqualsToken) ? AK_Definite : AK_Compound;
    }
    case TSK_PrefixUnaryExpression: case TSK_PostfixUnaryExpression: return AK_Compound;
    case TSK_ForInStatement: case TSK_ForOfStatement: return AK_Definite;
    default: return AK_None;
  }
}

static bool cIsFunctionOrSourceFile(TsNode *n) { return cIsFunctionLikeKind(n->kind) || n->kind == TSK_SourceFile; }
/* findAncestor (isFunctionOrSourceFile), the node itself included */
static TsNode *cFunctionOrSourceOf(TsNode *n) {
  for (; n; n = n->parent) if (cIsFunctionOrSourceFile(n)) return n;
  return NULL;
}

/* getRootDeclaration */
static TsNode *cRootDeclaration(TsNode *d) {
  while (d->kind == TSK_BindingElement && d->parent && d->parent->parent) d = d->parent->parent;
  return d;
}

/* isParameterOrMutableLocalVariable: a parameter, a catch variable, or a let variable that is not exported or global */
static bool cIsParamOrMutableLocal(TsSymbol *s) {
  TsNode *vd = s->valueDeclaration;
  if (!vd) return false;
  TsNode *d = cRootDeclaration(vd);
  if (d->kind == TSK_Parameter) return true;
  if (d->kind != TSK_VariableDeclaration || !d->parent) return false;
  if (d->parent->kind == TSK_CatchClause) return true;
  if (!(d->parent->flags & TNF_Let)) return false;
  if (bCombinedFlags(d, true) & TMF_Export) return false;
  if (d->parent->parent && d->parent->parent->kind == TSK_VariableStatement && d->parent->parent->parent &&
      d->parent->parent->parent->kind == TSK_SourceFile)
    return false;
  return true;
}

/* extendAssignmentPosition: the end of the statement that holds the assignment, when it is inside the declaration's container */
static int64_t cExtendAssignPos(TsNode *n, TsNode *decl) {
  int64_t pos = n->pos;
  for (TsNode *x = n; x && x->pos > decl->pos; x = x->parent) {
    switch (x->kind) {
      case TSK_VariableStatement: case TSK_ExpressionStatement: case TSK_IfStatement: case TSK_DoStatement:
      case TSK_WhileStatement: case TSK_ForStatement: case TSK_ForInStatement: case TSK_ForOfStatement:
      case TSK_WithStatement: case TSK_SwitchStatement: case TSK_TryStatement: case TSK_ClassDeclaration:
        pos = x->end;
        break;
      default: break;
    }
  }
  return pos;
}

/* markNodeAssignments: the last assignment position of each parameter or mutable local written under the node. An assignment
 * from a nested function, or a definite one, gives a negative or maximal position (the sign is kept as in TypeScript). */
static void cMarkNodeAssignments(TsNode *n) {
  if (n->kind == TSK_InterfaceDeclaration || n->kind == TSK_TypeAliasDeclaration || n->kind == TSK_EnumDeclaration) return;
  if (n->kind == TSK_Identifier) {
    int tk = cAssignmentTargetKind(n);
    if (tk == AK_None) return;
    TsSymbol *s = cResolveValueName(n, bnEscapeText(n->text, n->textLen));
    if (!s || !cIsParamOrMutableLocal(s)) return;
    bool def = tk == AK_Definite || (s->lastAssignSet && s->lastAssignPos < 0);
    if (!s->lastAssignSet || llabs(s->lastAssignPos) != CMAX_ASSIGN_POS) {
      TsNode *refF = cFunctionOrSourceOf(n), *declF = cFunctionOrSourceOf(s->valueDeclaration);
      s->lastAssignPos = refF == declF ? cExtendAssignPos(n, s->valueDeclaration) : CMAX_ASSIGN_POS;
      s->lastAssignSet = true;
    }
    if (def && s->lastAssignPos > 0) s->lastAssignPos = -s->lastAssignPos;
    return;
  }
  BNodeVec kids = { NULL, 0, 0 };
  bChildrenSorted(n, &kids);
  for (int i = 0; i < kids.n; i++) cMarkNodeAssignments(kids.v[i]);
  free(kids.v);
}

/* isPastLastAssignment: the location is after every assignment to the variable in its container (the container's assignments
 * are marked once, unless an enclosing one was marked already) */
static bool cIsPastLastAssignment(TsSymbol *s, TsNode *loc) {
  TsNode *fn = cFunctionOrSourceOf(s->valueDeclaration);
  if (!fn) return false;
  if (!fn->assignMarked) {
    fn->assignMarked = 1;
    bool marked = false;
    for (TsNode *p = fn->parent; p && !marked; p = p->parent)
      if (cIsFunctionOrSourceFile(p) && p->assignMarked) marked = true;
    if (!marked) cMarkNodeAssignments(fn);
  }
  if (!s->lastAssignSet || s->lastAssignPos == 0) return true;
  return llabs(s->lastAssignPos) < loc->pos;
}

/* getFlowTypeOfReference: the declared type when no narrowing applies, else the narrowed type (ported forms only). The
 * unreachable result and a non-null reference that is never not undefined give the declared type. */
static TsType *cFlowTypeOfReference(TsNode *ref, CFlowRef *r) {
  TsType *d = r->declared;
  if (!ref->flowNode || (d->flags & (TF_Undefined | TF_Null | TF_Void | TF_Never))) return d;
  r->nonUnion = !(d->flags & TF_Union);
  r->initial = r->startInit ? d : cAddUndefined(d);
  r->sharedStart = cSharedN;
  r->depth = 0;
  CFT ft = cFlowAt(ref->flowNode, r);
  cSharedN = r->sharedStart;
  TsType *res = ft.t;
  if (res == cUnreachableNeverType) return d;
  if (ref->parent && ref->parent->kind == TSK_NonNullExpression && !(res->flags & TF_Never) &&
      (cTypeWithFacts(res, TFC_NEUndefinedOrNull)->flags & TF_Never))
    return d;
  return res;
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

/* containsUndefinedType */
static bool cContainsUndefinedType(TsType *t) {
  if (t->flags & TF_Undefined) return true;
  if (t->flags & TF_Union) for (int i = 0; i < t->ntypes; i++) if (t->types[i]->flags & TF_Undefined) return true;
  return false;
}

/* isGenericType / isGenericIndexType of a type in the ported subset: 1 when generic, 0 when not, -1 when not decided here */
static int cGenericKind(TsType *t) {
  if (t == cErrorType) return -1;
  if (t->flags & (TF_Union | TF_Intersection)) {
    int any = 0;
    for (int i = 0; i < t->ntypes; i++) {
      int k = cGenericKind(t->types[i]);
      if (k < 0) return -1;
      if (k > 0) any = 1;
    }
    return any;
  }
  if (t->flags & (TF_TypeParameter | TF_IndexedAccess | TF_Conditional | TF_Index)) return 1;
  if ((t->flags & (TF_TemplateLiteral | TF_StringMapping)) && !cIsPatternLiteralType(t)) return 1;
  if (t->flags & TF_Object) {
    if (t->objectFlags & (OF_Mapped | OF_Tuple)) return -1;
    return 0;
  }
  return 0;
}

static TsType *cContextualType(TsNode *e);   /* below: getContextualType */

/* a call or new expression whose callee has a generic candidate (chooseOverload creates an inference context for it) */
static bool cCallHasGenericCandidate(TsNode *p) {
  TsType *f = cExprType(p->expression);
  if (f == cErrorType || !(f->flags & TF_Object)) return false;
  cResolveSignatures(f);
  int ns = p->kind == TSK_NewExpression ? f->nconstruct : f->ncall;
  CSig *sigs = p->kind == TSK_NewExpression ? f->construct : f->call;
  for (int i = 0; i < ns; i++) if (sigs[i].decl->typeParameters && sigs[i].decl->typeParameters->n) return true;
  return false;
}

/* getNarrowableTypeForReference (76095): the constraint of a type parameter with a union constraint replaces it when the
 * reference is in a constraint position (a property access, a call or new callee, an element access object whose argument
 * is not a generic index) or has a contextual type that is not generic. Returns 1 to substitute, 0 not, -1 not ported. */
static int cSubstituteConstraintFor(TsNode *e) {
  TsNode *p = e->parent;
  if (!p) return 0;
  if (p->kind == TSK_PropertyAccessExpression) return 1;
  if ((p->kind == TSK_CallExpression || p->kind == TSK_NewExpression) && p->expression == e) return 1;
  if (p->kind == TSK_ElementAccessExpression && p->expression == e) {
    TsType *at = cExprType(p->argumentExpression);
    if (at == cErrorType) return -1;
    int gk = cGenericKind(at);
    if (gk < 0) return -1;
    if (gk == 0) return 1;
  }
  /* an argument checked while its generic call is being resolved is checked in an inference context (checkMode Inferential),
   * which never substitutes the constraint */
  if ((p->kind == TSK_CallExpression || p->kind == TSK_NewExpression) && p->exprState == 1 && cCallHasGenericCandidate(p)) return 0;
  TsType *cx = cContextualType(e);
  if (cx == cErrorType) return -1;
  if (!cx) return 0;
  int gk = cGenericKind(cx);
  if (gk < 0) return -1;
  return gk == 0 ? 1 : 0;
}

/* the type of an identifier reference (checkIdentifier): the flow type of a variable, else the type of the symbol */
static TsType *cIdentifierType(TsNode *e) {
  BName nm = bnEscapeText(e->text, e->textLen);
  TsSymbol *s = cResolveValueName(e, nm);
  if (!s) {
    if (e->textLen == 9 && memcmp(e->text, "undefined", 9) == 0) return cUndefinedType;
    /* checkIdentifier: an unknown symbol is the error type (the name is reported by cCheckIdentifierReference) */
    return cErrorType;
  }
  if (s == cUndefinedSymbol) return cUndefinedType;
  if (!(s->flags & (SF_Variable | SF_Alias))) return cTypeOfSymbol(s);
  bool outer, startInit;
  TsType *d = cReferenceDeclaredType(e, s, &outer, &startInit);
  if (d == cErrorType) return d;
  if (d->flags & TF_TypeParameter) {
    /* isGenericTypeWithUnionConstraint: the base constraint is a union (or nullable) of types that are not generic */
    TsType *c = cConstraintOf(d);
    if (c && (c->flags & (TF_Union | TF_Null | TF_Undefined))) {
      int gc = cGenericKind(c);
      if (gc != 0) return cUnported("reference of a type parameter with a union constraint");
      int sub = cSubstituteConstraintFor(e);
      if (sub < 0) return cUnported("reference of a type parameter with a union constraint");
      if (sub > 0) d = c;
    }
  }
  /* checkIdentifier: the flow container moves up past the function expressions, arrows and object-literal methods that
   * create the reference, for a const variable, and for a parameter or mutable local past its last assignment. The walk
   * then continues at the creation flow of those function values (jump). */
  TsNode *decl = cFirstDeclOf(s);
  TsNode *ownC = cFlowContainerOf(e);
  TsNode *flowC = ownC;
  if (outer && decl) {
    TsNode *declC = cFlowContainerOf(decl);
    bool isConst = decl->kind == TSK_VariableDeclaration && decl->parent && decl->parent->kind == TSK_VariableDeclarationList &&
                   (decl->parent->flags & 7u) == TNF_Const;
    while (flowC && flowC != declC &&
           (flowC->kind == TSK_FunctionExpression || flowC->kind == TSK_ArrowFunction ||
            ((flowC->kind == TSK_MethodDeclaration || flowC->kind == TSK_GetAccessor || flowC->kind == TSK_SetAccessor) &&
             (bContainerFlags(flowC) & CF_IsObjectLiteralOrClassExpressionMethodOrAccessor))) &&
           (isConst || (cIsParamOrMutableLocal(s) && cIsPastLastAssignment(s, e))))
      flowC = cFlowContainerOf(flowC);
  }
  bool jump = flowC != ownC;
  CFlowRef r = { e, e->text, e->textLen, d, false, outer, startInit, jump, flowC };
  TsType *ft = cFlowTypeOfReference(e, &r);
  /* checkIdentifier: a reference that is not assumed initialized and whose flow type has undefined that its declared type
   * has not is an error (2454), and it has its declared type */
  if (!startInit && ft != cErrorType && cContainsUndefinedType(ft) && !cContainsUndefinedType(d)) {
    cDiagAdd(cSourceFileOf(e), cNodeStart(e), 2454, NULL);
    return d;
  }
  return ft;
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
  CFlowRef r = { e, text, len, declared, false, false, startInit, false, NULL, false };
  return cFlowTypeOfReference(e, &r);
}

/* ObjectFlags of an array literal type (createArrayLiteralType): the ArrayLiteral flag of a clone */
#define OF_ArrayLiteral 0x2000

/* ---- contextual types (getContextualType): NULL when there is none, the error type when it is not ported ---- */
static TsType *cContextualReturnOf(TsNode *fn);
static TsType *cAssignmentTargetType(TsNode *left);
static TsType *cApparentOf(TsType *o);
struct CCallInfer;
static struct CCallInfer *cInferenceContextOf(TsNode *node);
static TsType *cInferFixContext(struct CCallInfer *ic, TsType *pt);
static TsType *cGenericCallFnArgContext(CSig *sig, int idx, TsNode *arg);
/* the contextual type of a function argument while the candidate of an overloaded call is checked (chooseOverload's
 * second round): the parameter type of that candidate. Saved and restored around each argument check. */
static TsNode *cCandFn;
static TsType *cCandType;

static bool cIsInitializedDeclParam(TsNode *p);

/* isTupleLikeType of a contextual type: a tuple, an object with a "0" property, or a union with such a member (someType) */
static bool cIsTupleLikeContext(TsType *ac) {
  if (ac->flags & TF_Union) {
    for (int i = 0; i < ac->ntypes; i++) if (cIsTupleLikeContext(ac->types[i])) return true;
    return false;
  }
  if (!(ac->flags & TF_Object)) return false;
  return cIsTupleType(ac) || cPropertyOf(ac, bnC("0")) != NULL;
}

/* getContextualTypeForElementExpression of an array context: any gives any, an array the type of its number index, and a
 * union the element context of its one object member once undefined and null are dropped (several members are not ported) */
static TsType *cArrayElementContext(TsType *ac) {
  if (ac->flags & TF_Any) return ac;
  if (ac->flags & TF_Union) {
    TsType *one = NULL;
    for (int i = 0; i < ac->ntypes; i++) {
      if (ac->types[i]->flags & (TF_Undefined | TF_Null)) continue;
      if (one) return cUnported("contextual element of a union of several arrays");
      one = ac->types[i];
    }
    return one ? cArrayElementContext(one) : NULL;
  }
  /* an unconstrained type parameter or unknown has no element: getIteratedTypeOrElementType and the properties of a
   * non-structured type give nothing (a constrained parameter takes its apparent type, not ported) */
  if (ac->flags & TF_Unknown) return NULL;
  if (ac->flags & TF_TypeParameter) {
    if (!cConstraintOf(ac)) return NULL;
    return cUnported("contextual element of a constrained type parameter");
  }
  if (!(ac->flags & TF_Object)) return cUnported("contextual element of a non-object");
  cResolveIndexInfos(ac);
  for (int i = 0; i < ac->nindex; i++) if (ac->index[i].keyType == cNumberType) return ac->index[i].type;
  return cUnported("contextual element without a number index");
}

/* isLiteralType: a boolean, an enum literal union, a union of unit types, or a unit type */
static bool cIsLiteralTypeOf(TsType *t) {
  if (t->flags & TF_Boolean) return true;
  if (t->flags & TF_Union) {
    if (t->flags & TF_EnumLiteral) return true;
    for (int i = 0; i < t->ntypes; i++) if (!cIsUnitType(t->types[i])) return false;
    return true;
  }
  return cIsUnitType(t);
}

/* isPossiblyDiscriminantValue: a literal, an identifier, a template expression or a property access of one */
static bool cPossiblyDiscriminantValue(TsNode *v) {
  switch (v->kind) {
    case TSK_StringLiteral: case TSK_NumericLiteral: case TSK_BigIntLiteral: case TSK_NoSubstitutionTemplateLiteral:
    case TSK_TemplateExpression: case TSK_TrueKeyword: case TSK_FalseKeyword: case TSK_NullKeyword: case TSK_Identifier:
      return true;
    case TSK_PropertyAccessExpression: case TSK_ParenthesizedExpression: return cPossiblyDiscriminantValue(v->expression);
    default: return false;
  }
}

/* discriminateContextualTypeByObjectMembers over a union contextual type of an object literal: the properties of the literal
 * that are discriminants of the union (isDiscriminantProperty: the members' types differ and one is a literal type) with
 * their value types narrow the members (discriminateTypeByDiscriminableItems, related by assignability). A member that
 * lacks a discriminant property is not ported. Returns u itself when nothing is excluded. */
static TsType *cDiscriminatedUnion(TsType *u, TsNode *lit) {
  int nt = u->ntypes;
  int *incl = ts_arena_alloc(sizeof(int) * (size_t)(nt ? nt : 1));   /* 1 included, 0 excluded, 2 maybe */
  for (int k = 0; k < nt; k++) {
    if (!(u->types[k]->flags & TF_Object)) return cUnported("discriminant of a union with a non-object member");
    incl[k] = 1;
  }
  if (!lit->properties) return u;
  for (int i = 0; i < lit->properties->n; i++) {
    TsNode *pr = lit->properties->v[i];
    bool cand = pr->kind == TSK_ShorthandPropertyAssignment ||
                (pr->kind == TSK_PropertyAssignment && pr->initializer && cPossiblyDiscriminantValue(pr->initializer));
    if (!cand) continue;
    if (!pr->name || pr->name->kind != TSK_Identifier) return cUnported("discriminant of an object literal");
    BName name = bnEscapeText(pr->name->text, pr->name->textLen);
    /* isDiscriminantProperty over every member (a member without the property is not ported) */
    TsType *first = NULL;
    bool nonUniform = false, literal = false;
    for (int k = 0; k < nt; k++) {
      TsSymbol *ps = cPropertyOf(u->types[k], name);
      if (!ps) { cResolveIndexInfos(u->types[k]); if (u->types[k]->nindex) return cUnported("discriminant of a union with an index signature"); return cUnported("discriminant of a union with a partial property"); }
      TsType *t = cTypeOfSymbol(ps);
      if (t == cErrorType) return t;
      if (first && !cIdentical(t, first)) nonUniform = true;
      if (!first) first = t;
      if (cIsLiteralTypeOf(t)) literal = true;
    }
    if (!(nonUniform && literal)) continue;
    /* the value type of the discriminant (getContextFreeTypeOfExpression of the initializer or the name) */
    TsType *vt = cExprType(pr->kind == TSK_ShorthandPropertyAssignment ? pr->name : pr->initializer);
    if (vt == cErrorType) return vt;
    bool matched = false;
    for (int k = 0; k < nt; k++) {
      if (!incl[k]) continue;
      TsSymbol *ps = cPropertyOf(u->types[k], name);
      TsType *target = cTypeOfSymbol(ps);
      if (cIsRelatedWith(vt, target, REL_Assignable)) matched = true;
      else incl[k] = 2;
    }
    for (int k = 0; k < nt; k++) if (incl[k] == 2) incl[k] = matched ? 0 : 1;
  }
  TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)(nt ? nt : 1));
  int n = 0;
  for (int k = 0; k < nt; k++) if (incl[k]) kept[n++] = u->types[k];
  if (n == nt || n == 0) return u;
  return n == 1 ? kept[0] : cGetUnionType(kept, n);
}

/* getTypeOfPropertyOfContextualType over a union contextual type of an object literal (after its discriminants): the union of
 * the members' property types with that name (a member without it adds nothing). A member with an index signature that could
 * apply is not ported. */
static TsType *cUnionPropertyContext(TsType *u, BName name) {
  TsType **types = ts_arena_alloc(sizeof(TsType *) * (size_t)(u->ntypes ? u->ntypes : 1));
  int n = 0;
  for (int k = 0; k < u->ntypes; k++) {
    TsType *m = u->types[k];
    TsSymbol *ps = cPropertyOf(m, name);
    if (ps) {
      TsType *t = cTypeOfSymbol(ps);
      if (t == cErrorType) return t;
      types[n++] = t;
    } else {
      cResolveIndexInfos(m);
      if (m->nindex) return cUnported("contextual type of a property of a union with an index signature");
    }
  }
  if (n == 0) return NULL;
  return n == 1 ? types[0] : cGetUnionType(types, n);
}

/* the contextual type of the property named by a member of an object literal whose contextual type is oc */
static TsType *cObjectMemberContext(TsType *oc, TsNode *lit, TsNode *nameNode) {
  if (oc->flags & TF_Any) return NULL;   /* a property of any is not a structured type's property: no contextual type */
  BName name = bnEscapeText(nameNode->text, nameNode->textLen);
  if (oc->flags & TF_Union) {
    TsType *d = cDiscriminatedUnion(oc, lit);
    if (d == cErrorType) return d;
    if (d->flags & TF_Union) return cUnionPropertyContext(d, name);
    oc = d;
  }
  if (oc->flags & TF_Any) return NULL;
  if (!(oc->flags & TF_Object)) return cUnported("contextual type of a property of a non-object");
  TsSymbol *ps = cPropertyOf(oc, name);
  return ps ? cTypeOfSymbol(ps) : NULL;
}

static TsType *cContextualType(TsNode *e) {
  if (cCandFn && e == cCandFn) return cCandType;
  TsNode *c = e, *p = e->parent;
  while (p && p->kind == TSK_ParenthesizedExpression) { c = p; p = p->parent; }
  if (!p) return NULL;
  switch (p->kind) {
    case TSK_VariableDeclaration:
      if (p->initializer != c) return cUnported("contextual type of a declaration name");
      return p->type ? cTypeFromNode(p->type) : NULL;
    case TSK_Parameter:
      /* getContextualTypeForInitializerExpression: a parameter of a function that is not contextually typed has none */
      if (p->initializer != c) return cUnported("contextual type of a parameter name");
      if (cIsInitializedDeclParam(p)) return NULL;
      return cUnported("contextual type");
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
      /* an untyped call (a callee of type any) resolves to anySignature, which has no parameters: every argument is any
       * (getTypeAtPosition of anySignature). An unannotated function argument then has no contextual signature */
      if (f == cAnyType) return cAnyType;
      if (!(f->flags & TF_Object)) return cUnported("contextual type of a call of a non-object");
      cResolveSignatures(f);
      int ns = p->kind == TSK_NewExpression ? f->nconstruct : f->ncall;
      CSig *sigs = p->kind == TSK_NewExpression ? f->construct : f->call;
      CSig *sig;
      if (p->resolvedSig) {
        /* getContextualTypeForArgumentAtIndex: the parameter of the resolved signature of the call (links.resolvedSignature) */
        sig = p->resolvedSig;
      } else {
        if (ns > 1) {
          /* the parameter of the chosen signature is only known after the overload is resolved: an argument whose type does not
           * depend on its contextual type has none (an identifier, a literal, a property access) */
          TsNode *ca = cSkipParens(c);
          switch (ca->kind) {
            case TSK_Identifier: case TSK_NumericLiteral: case TSK_StringLiteral: case TSK_NoSubstitutionTemplateLiteral:
            case TSK_NullKeyword: case TSK_TrueKeyword: case TSK_FalseKeyword: case TSK_PropertyAccessExpression:
              return NULL;
            default: return cUnported("contextual type of a call with several signatures");
          }
        }
        if (ns != 1) return cUnported("contextual type of a call with several signatures");
        sig = &sigs[0];
        if (sig->decl->typeParameters && sig->decl->typeParameters->n) {
          TsNode *ca = cSkipParens(c);
          if (ca->kind == TSK_ObjectLiteralExpression) {
            /* getContextualTypeForArgument of an object literal in an inference: the parameter type, uninstantiated
             * (instantiateContextualType instantiates only for a signature). The literal's type does not depend on it;
             * a property that needs it (a literal or a function value) meets a non-object context and is not ported */
            CParams gp;
            bool gok;
            if (!cSplitParams(sig->decl, &gp) || gp.thisParam || gp.rest) return cUnported("contextual type of a generic call");
            TsType *gt = cSigParamType(sig, &gp, idx, &gok);
            if (!gok) return cUnported("contextual type of a generic call");
            return gt;
          }
          if (ca->kind != TSK_ArrowFunction) return cUnported("contextual type of a generic call");
          return cGenericCallFnArgContext(sig, idx, ca);
        }
      }
      CParams ps;
      if (!cSplitParams(sig->decl, &ps) || ps.thisParam) return cUnported("contextual type of a call with a this parameter");
      bool ok;
      TsType *pt = cSigParamType(sig, &ps, idx, &ok);
      return ok ? pt : NULL;
    }
    case TSK_TemplateSpan: return NULL;   /* a template span has no contextual type */
    case TSK_ObjectLiteralExpression: {
      /* getContextualTypeForObjectLiteralMethod: the property of the object literal's contextual type named by the method */
      if (c->kind != TSK_MethodDeclaration || !c->name || c->name->kind != TSK_Identifier) return cUnported("contextual type of an object literal member");
      TsType *oc = cContextualType(p);
      if (oc == NULL || oc == cErrorType) return oc;
      return cObjectMemberContext(oc, p, c->name);
    }
    case TSK_PropertyAccessExpression: case TSK_ElementAccessExpression:
      return NULL;   /* getContextualType2 has no case for an access: its object and its argument have none */
    case TSK_ShorthandPropertyAssignment: case TSK_PropertyAssignment: {
      /* the property of the contextual type of the object literal (getContextualTypeForObjectLiteralElement) */
      TsNode *value = p->kind == TSK_ShorthandPropertyAssignment ? p->name : p->initializer;
      if (value != c || !p->name || p->name->kind != TSK_Identifier || !p->parent || p->parent->kind != TSK_ObjectLiteralExpression)
        return cUnported("contextual type of a property name");
      TsType *oc = cContextualType(p->parent);
      if (oc == NULL || oc == cErrorType) return oc;
      return cObjectMemberContext(oc, p->parent, p->name);
    }
    case TSK_ArrayLiteralExpression: {
      TsType *ac = cContextualType(p);
      if (ac == NULL || ac == cErrorType) return ac;
      if (cIsTupleLikeContext(ac)) {
        if (!cIsTupleType(ac)) return cUnported("contextual element of a tuple-like context");
        /* getContextualTypeForElementExpression of a tuple: the element type at the index (none beyond the arity) */
        int idx = 0;
        while (idx < p->elements->n && p->elements->v[idx] != c) idx++;
        return idx < ac->nargs ? ac->args[idx] : NULL;
      }
      return cArrayElementContext(ac);
    }
    case TSK_ConditionalExpression:
      if (p->condition == c) return NULL;
      return cContextualType(p);
    case TSK_BinaryExpression: {
      /* getContextualTypeForAssignmentDeclaration of `target = value` in a TypeScript file: the type of the target, unless the
       * target is a property declaration with neither a type nor an initializer */
      if (p->operatorToken->kind != TSK_EqualsToken || p->right != c) return cUnported("contextual type");
      TsNode *l = cSkipParens(p->left);
      if (l->kind == TSK_Identifier) return cAssignmentTargetType(l);
      if (l->kind == TSK_ElementAccessExpression) {
        /* getContextualTypeForAssignmentDeclaration (kind None): the symbol of a target whose key is a property name (a literal
         * or unique symbol) decides; any other target gives the type of the target (getTypeOfExpression) */
        if (!l->argumentExpression) return cUnported("contextual type");
        TsType *kt = cExprType(l->argumentExpression);
        if (kt == cErrorType) return kt;
        if (kt->flags & (TF_StringLiteral | TF_NumberLiteral | TF_UniqueESSymbol)) return cUnported("contextual type of an assignment to a property name");
        return cExprType(l);
      }
      if (l->kind != TSK_PropertyAccessExpression || !l->name || l->name->kind != TSK_Identifier) return cUnported("contextual type");
      TsType *lt = cAssignmentTargetType(l);
      if (lt == cErrorType) return lt;
      TsType *o = cApparentOf(cExprType(l->expression));
      TsSymbol *ps = o != cErrorType && (o->flags & TF_Object) ? cPropertyOf(o, bnEscapeText(l->name->text, l->name->textLen)) : NULL;
      TsNode *decl = ps && ps->valueDeclaration ? ps->valueDeclaration : NULL;
      if (decl && decl->kind == TSK_PropertyDeclaration && !decl->type && !decl->initializer) return NULL;
      return lt;
    }
    default: break;
  }
  return cUnported("contextual type");
}

/* getContextualCallSignature of an object type: the one call signature that is not too small for the function (isAritySmaller);
 * none (NULL without *err) when no call signature is applicable. A single signature result is a leaf; several are combined
 * (getIntersectedSignatures, reduceLeft of combineSignaturesOfIntersectionMembers, which the strict options of the benchmark
 * programs enable: noImplicitAny). A generic, a this parameter, a rest or an initialized parameter in a combination, and every
 * other context not ported set *err. */
/* A contextual signature: a leaf is one signature of the contextual type; a combined node is the synthetic signature of
 * combineSignaturesOfIntersectionMembers (its parameter types are the union of the members' at each position, its return type
 * the intersection of theirs). Combined signatures only exist for the contextual parameter types and the contextual return
 * type (cContextualSigNode): any other use of one is not ported. */
struct CCombinedSig { CSig *leaf; struct CCombinedSig *left, *right; int count, min; };
typedef struct CCombinedSig CCombinedSig;

static CCombinedSig *cLeafSigNode(CSig *s, bool *err) {
  CParams cp;
  if (!cSplitParams(s->decl, &cp)) { *err = true; return NULL; }
  CCombinedSig *c = ts_arena_alloc(sizeof(CCombinedSig));
  c->leaf = s;
  c->left = c->right = NULL;
  c->count = cp.n;
  c->min = cp.min;
  return c;
}

/* a signature a combination is made of: no this parameter, rest parameter, type parameter or initializer */
static bool cCombinableSig(CSig *s) {
  CParams cp;
  if (!cSplitParams(s->decl, &cp) || cp.thisParam || cp.rest) return false;
  if (s->decl->typeParameters && s->decl->typeParameters->n) return false;
  for (int i = 0; i < cp.n; i++) if (cp.params[i]->initializer) return false;
  return true;
}

/* combineIntersectionParameters and the counts of combineSignaturesOfIntersectionMembers: the longer count and the larger
 * minimum (getMinArgumentCount of the combination is the maximum of the two) */
static CCombinedSig *cCombineSigNodes(CCombinedSig *l, CCombinedSig *r) {
  CCombinedSig *c = ts_arena_alloc(sizeof(CCombinedSig));
  c->leaf = NULL;
  c->left = l;
  c->right = r;
  c->count = l->count > r->count ? l->count : r->count;
  c->min = l->min > r->min ? l->min : r->min;
  return c;
}

/* getTypeAtPosition of a contextual signature, with the ok flag of tryGetTypeAtPosition (no parameter at the position).
 * A combined position is the union of the members' types there (getUnionType of the longest member's type and the
 * shorter member's, unknown past the end of the shorter one), longest first, as combineIntersectionParameters builds it. */
static TsType *cCombinedParamType(CCombinedSig *c, int pos, bool *ok) {
  *ok = pos >= 0 && pos < c->count;
  if (!*ok) return NULL;
  if (c->leaf) {
    CParams cp;
    if (!cSplitParams(c->leaf->decl, &cp)) { *ok = false; return cErrorType; }
    return cSigParamType(c->leaf, &cp, pos, ok);
  }
  CCombinedSig *longest = c->left->count >= c->right->count ? c->left : c->right;
  CCombinedSig *shorter = longest == c->left ? c->right : c->left;
  bool okL = false, okS = false;
  TsType *lt = cCombinedParamType(longest, pos, &okL);
  TsType *st = cCombinedParamType(shorter, pos, &okS);
  if (!okS) st = cUnknownType;
  if (!okL) { *ok = false; return NULL; }
  TsType *pair[2] = { lt, st };
  return cGetUnionType(pair, 2);
}

/* the return type of a contextual signature: a leaf's, or the intersection of the combined leaves' in their order
 * (getUnionOrIntersectionType of the composite signatures with the intersection kind) */
static int cCountLeafSigs(CCombinedSig *c) {
  return c->leaf ? 1 : cCountLeafSigs(c->left) + cCountLeafSigs(c->right);
}
static void cCollectLeafSigs(CCombinedSig *c, CSig **out, int *n) {
  if (c->leaf) { out[(*n)++] = c->leaf; return; }
  cCollectLeafSigs(c->left, out, n);
  cCollectLeafSigs(c->right, out, n);
}
static TsType *cCombinedReturn(CCombinedSig *c) {
  if (c->leaf) return cReturnTypeOf(c->leaf);
  int nleaf = 0;
  CSig **leaves = ts_arena_alloc(sizeof(CSig *) * (size_t)cCountLeafSigs(c));
  cCollectLeafSigs(c, leaves, &nleaf);
  TsType **rs = ts_arena_alloc(sizeof(TsType *) * (size_t)nleaf);
  for (int i = 0; i < nleaf; i++) rs[i] = cReturnTypeOf(leaves[i]);
  return cGetIntersectionType(rs, nleaf);
}

static CCombinedSig *cContextualCallNode(TsNode *fn, TsType *ctx, bool *err) {
  /* isAritySmaller: the target parameters before the first optional, initialized or rest one (a this parameter is not counted) */
  TsList *pl = fn->parameters;
  int target = 0;
  while (pl && target < pl->n && !pl->v[target]->initializer && !pl->v[target]->questionToken && !pl->v[target]->dotDotDotToken) target++;
  if (pl && pl->n && cIsThisParam(pl->v[0])) target--;
  CSig **app = ts_arena_alloc(sizeof(CSig *) * (size_t)(ctx->ncall ? ctx->ncall : 1));
  int applicable = 0;
  for (int c = 0; c < ctx->ncall; c++) {
    CSig *s = &ctx->call[c];
    CParams sp;
    if (!cSplitParams(s->decl, &sp) || sp.thisParam) { *err = true; return NULL; }
    if (!sp.rest && sp.n < target) continue;
    app[applicable++] = s;
  }
  if (applicable == 0) return NULL;
  if (applicable == 1) {
    if (app[0]->decl->typeParameters && app[0]->decl->typeParameters->n) { *err = true; return NULL; }
    return cLeafSigNode(app[0], err);
  }
  /* getIntersectedSignatures: reduceLeft over the applicable signatures, each combined with the next one */
  CCombinedSig *acc = NULL;
  for (int k = 0; k < applicable; k++) {
    if (!cCombinableSig(app[k])) { *err = true; return NULL; }
    CCombinedSig *leaf = cLeafSigNode(app[k], err);
    if (*err) return NULL;
    acc = acc ? cCombineSigNodes(acc, leaf) : leaf;
  }
  return acc;
}

/* the one call signature of an object context as a single signature (a union member): a combination is not ported there
 * (createUnionSignature and compareSignaturesIdentical) */
static CSig *cContextualCallSignatureOf(TsNode *fn, TsType *ctx, bool *err) {
  CCombinedSig *n = cContextualCallNode(fn, ctx, err);
  if (!n) return NULL;
  if (!n->leaf) { *err = true; return NULL; }
  return n->leaf;
}

/* getContextualSignature of a function expression or arrow: the contextual signature (a leaf or a combined node). An object
 * type with call signatures gives its own (getContextualCallSignature); a union gives the one signature of its members that
 * have call signatures (members without any, and undefined, null and the primitives, have none). Several signatures of a
 * union (createUnionSignature), a generic, a this parameter and every context not ported set *err (NULL with *err); a NULL
 * result without *err means there is no contextual signature. */
static CCombinedSig *cContextualSigNode(TsNode *fn, bool *err) {
  *err = false;
  TsType *ctx = cContextualType(fn);
  if (ctx == NULL) return NULL;
  if (ctx == cErrorType) { *err = true; return NULL; }
  if (ctx->flags & TF_Any) return NULL;   /* getContextualCallSignature of any: no call signatures, so no contextual signature */
  if (ctx->flags & TF_Union) {
    CSig *found = NULL;
    int nfound = 0;
    for (int i = 0; i < ctx->ntypes; i++) {
      TsType *m = ctx->types[i];
      if (!(m->flags & TF_Object)) {
        /* a primitive or unit member has no call signatures (its apparent type has none) */
        if (m->flags & (TF_Union | TF_Intersection | TF_TypeParameter | TF_Index | TF_IndexedAccess | TF_Conditional |
                        TF_TemplateLiteral | TF_StringMapping | TF_NonPrimitive)) { *err = true; return NULL; }
        continue;
      }
      cResolveSignatures(m);
      if (m->ncall == 0) continue;
      bool e2 = false;
      CSig *s = cContextualCallSignatureOf(fn, m, &e2);
      if (e2) { *err = true; return NULL; }
      if (s) { found = s; nfound++; }
    }
    if (nfound > 1) { *err = true; return NULL; }   /* createUnionSignature (or no signature when the members differ) */
    if (!found) return NULL;
    return cLeafSigNode(found, err);
  }
  if (!(ctx->flags & TF_Object)) { *err = true; return NULL; }
  cResolveSignatures(ctx);
  if (ctx->ncall == 0) { *err = true; return NULL; }
  return cContextualCallNode(fn, ctx, err);
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
  CCombinedSig *cn = cContextualSigNode(fn, &err);
  if (err) return cErrorType;
  return cn ? cCombinedReturn(cn) : NULL;
}

/* a type with a literal or type variable member: a contextual type that keeps a literal return (isLiteralOfContextualType) */
static bool cHasLiteralOrVariable(TsType *t) {
  if (t->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BooleanLiteral | TF_BigIntLiteral | TF_TypeParameter |
                  TF_IndexedAccess | TF_Conditional | TF_Index | TF_TemplateLiteral | TF_StringMapping)) return true;
  if (t->flags & TF_Union) for (int i = 0; i < t->ntypes; i++) if (cHasLiteralOrVariable(t->types[i])) return true;
  return false;
}

/* a parameter of a method or function declaration without annotation, initializer, optional or rest marker: its type is any
 * (getContextuallyTypedParameterType is undefined for these, and the implicit any is not reported without noImplicitAny).
 * An object literal method is context sensitive, and so is a `this` parameter: not covered here. */
static bool cIsUntypedDeclParam(TsNode *p) {
  TsNode *fn = p->parent;
  if (!fn || (fn->kind != TSK_MethodDeclaration && fn->kind != TSK_FunctionDeclaration)) return false;
  if (p->type || p->initializer || p->questionToken || p->dotDotDotToken) return false;
  if (!p->name || p->name->kind != TSK_Identifier || (p->name->textLen == 4 && memcmp(p->name->text, "this", 4) == 0)) return false;
  return !(fn->parent && fn->parent->kind == TSK_ObjectLiteralExpression);
}

/* getContextuallyTypedParameterType and assignContextualParameterTypes: an unannotated parameter of a function expression or
 * arrow has the type at its position in the contextual signature. Parameters with an initializer, optional or rest
 * parameters, binding patterns and the parameters of other functions (implicit any) are not ported. */
/* a parameter without annotation and with an initializer, of a function declaration or of a method that is not an object
 * literal member: it has no contextual type (getContextuallyTypedParameterType is undefined), so its type is the widened
 * type of the initializer (getTypeForVariableLikeDeclaration) */
static bool cIsInitializedDeclParam(TsNode *p) {
  TsNode *fn = p->parent;
  if (!fn || p->type || !p->initializer || p->questionToken || p->dotDotDotToken) return false;
  if (!p->name || p->name->kind != TSK_Identifier || (p->name->textLen == 4 && memcmp(p->name->text, "this", 4) == 0)) return false;
  if (fn->kind != TSK_FunctionDeclaration && fn->kind != TSK_MethodDeclaration) return false;
  return !(fn->parent && fn->parent->kind == TSK_ObjectLiteralExpression);
}

static TsType *cUnannotatedParamType(TsSymbol *sym, TsNode *d) {
  (void)sym;
  if (cIsUntypedDeclParam(d)) return cAnyType;
  if (cIsInitializedDeclParam(d)) {
    /* getTypeOfParameter: the initializer's widened type, with undefined (the parameter is optional) */
    if (d->inferState == 1) return cUnported("circular parameter type");
    d->inferState = 1;
    TsType *t = cExprType(d->initializer);
    if (t != cErrorType) t = cAddUndefined(cMutableLocationType(d->initializer, t));
    d->inferState = 0;
    return t;
  }
  TsNode *fn = d->parent;
  /* an object literal method is context sensitive too (isContextSensitiveFunctionOrObjectLiteralMethod) */
  bool objMethod = fn && fn->kind == TSK_MethodDeclaration && fn->parent && fn->parent->kind == TSK_ObjectLiteralExpression;
  if (!fn || (fn->kind != TSK_FunctionExpression && fn->kind != TSK_ArrowFunction && !objMethod)) return cUnported("parameter without annotation");
  if (d->initializer || d->questionToken || d->dotDotDotToken || !d->name || d->name->kind != TSK_Identifier)
    return cUnported("parameter without annotation");
  if (d->inferState == 1) return cUnported("circular parameter type");
  d->inferState = 1;
  /* the refusal is counted only when no type is found: a counted placeholder would end the inference or narrowing around it */
  TsType *t = NULL;
  bool err = false;
  CCombinedSig *cn = cContextualSigNode(fn, &err);
  if (!err && cn) {
    int idx = 0;
    while (idx < fn->parameters->n && fn->parameters->v[idx] != d) idx++;
    if (fn->parameters->n && cIsThisParam(fn->parameters->v[0])) idx--;
    if (idx >= 0) {
      bool ok;
      TsType *pt = cCombinedParamType(cn, idx, &ok);
      /* a position beyond the contextual parameters has no type (tryGetTypeAtPosition): implicit any */
      if (ok) t = pt;
    }
  }
  d->inferState = 0;
  if (!t || t == cErrorType) return cUnported("parameter without annotation");
  return t;
}

/* the contextual type of a parameter with a binding pattern (getContextuallyTypedParameterType): the parameter at its position
 * in the contextual signature of its function expression or arrow. Other functions, initializers and rest parameters are not
 * ported */
static TsType *cContextualParamOfPattern(TsNode *d) {
  TsNode *fn = d->parent;
  if (!fn || (fn->kind != TSK_FunctionExpression && fn->kind != TSK_ArrowFunction)) return cUnported("binding element of a parameter");
  if (d->initializer || d->questionToken || d->dotDotDotToken) return cUnported("binding element of a parameter");
  if (d->inferState == 1) return cUnported("circular parameter type");
  d->inferState = 1;
  bool err = false;
  CCombinedSig *cn = cContextualSigNode(fn, &err);
  TsType *t = NULL;
  if (!err && cn) {
    int idx = 0;
    while (idx < fn->parameters->n && fn->parameters->v[idx] != d) idx++;
    if (fn->parameters->n && cIsThisParam(fn->parameters->v[0])) idx--;
    bool ok = false;
    if (idx >= 0) {
      TsType *pt = cCombinedParamType(cn, idx, &ok);
      if (ok) t = pt;
    }
  }
  d->inferState = 0;
  if (!t || t == cErrorType) return t ? t : cUnported("binding element of a parameter");
  return t;
}

/* getTypeForBindingElement: a destructured variable has the property of an object pattern, or the element of an array pattern,
 * of the type of the declaration the pattern belongs to (getTypeForBindingElementParent). Ported: a declaration with a type
 * annotation or an initializer (widened as a mutable location), nested patterns, identifier property names, array elements of
 * an array type, the element of a tuple at its position. Defaults, rest elements, parameters other than a contextually typed
 * one, and missing properties are not ported. */
static TsType *cBindingElementType(TsSymbol *sym, TsNode *d) {
  TsNode *pat = d->parent;
  TsNode *decl = pat ? pat->parent : NULL;
  if (!pat || !decl || !pat->elements || d->dotDotDotToken || d->initializer) return cUnported("binding element");
  TsType *pt;
  if (decl->kind == TSK_VariableDeclaration) {
    if (decl->type) pt = cTypeFromNode(decl->type);
    else if (decl->initializer) {
      TsType *t = cExprType(decl->initializer);
      if (t == cErrorType) return t;
      pt = cIsConstVariableDecl(decl) ? t : cMutableLocationType(decl->initializer, t);
    } else return cUnported("binding element of a declaration without a type");
  } else if (decl->kind == TSK_BindingElement && decl->symbol) {
    pt = cTypeOfSymbol(decl->symbol);
  } else if (decl->kind == TSK_Parameter) {
    /* getTypeForVariableLikeDeclaration of a parameter with a binding pattern: its annotation, else its contextual parameter type */
    if (decl->type) pt = cTypeFromNode(decl->type);
    else pt = cContextualParamOfPattern(decl);
  } else {
    return cUnported("binding element of a parameter");
  }
  if (pt == cErrorType) return pt;
  if (pat->kind == TSK_ObjectBindingPattern) {
    TsNode *nm = d->propertyName ? d->propertyName : d->name;
    if (!nm || nm->kind != TSK_Identifier) return cUnported("binding element name");
    if (!(pt->flags & TF_Object)) return cUnported("binding element of a non-object");
    TsSymbol *ps = cPropertyOf(pt, bnEscapeText(nm->text, nm->textLen));
    if (!ps) return cUnported("binding element without a property");
    return cTypeOfSymbol(ps);
  }
  /* an array pattern: the element at the position of the binding element (an omitted element counts) */
  if (pat->kind != TSK_ArrayBindingPattern) return cUnported("binding element pattern");
  if (cIsTupleType(pt)) {
    /* getIndexedAccessType of the tuple at the index of the element (no rest element or element past the arity) */
    int idx = 0;
    while (idx < pat->elements->n && pat->elements->v[idx] != d) idx++;
    if (idx < pt->nargs) return pt->args[idx];
    return cUnported("array binding past the arity of a tuple");
  }
  if (!(pt->flags & TF_Object)) return cUnported("array binding of a non-array");
  cResolveIndexInfos(pt);
  for (int i = 0; i < pt->nindex; i++) if (pt->index[i].keyType == cNumberType) {
    (void)sym;
    return pt->index[i].type;
  }
  return cUnported("array binding without an element type");
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
  if (cLoopN) cFlowLogAdd(e);   /* computed while a loop back edge is walked: dropped with the back edge (cLoopPop) */
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

/* getUnionOrIntersectionProperty of a union (ts-types.h): the property every member has, NULL when none */
static TsSymbol *cUnionProperty(TsType *u, BName name) { return cGetPropertyOfUnionOrIntersection(u, name); }

/* a property of an object type, or the error type */
static TsType *cMemberType(TsType *o, TsSymbol *p, TsNode *e) {
  (void)o;
  if (!p) return cUnported("member not found");
  (void)e;
  return cTypeOfSymbol(p);
}

/* the symbols of the global interfaces that give the members of every object: Object, CallableFunction (an object with call
 * signatures, strictBindCallApply) and NewableFunction (one with construct signatures). Their own declared members are read
 * from the binder table (none of these interfaces has a base to inherit from). */
static TsSymbol *cGlobalMemberIface(int idx) {
  static const char *const names[3] = { "Object", "CallableFunction", "NewableFunction" };
  return bTableGet(cGlobals, bnC(names[idx]));   /* not cached: the globals are rebuilt by every checker run */
}

/* getPropertyOfType of an object type (the non-union case): its member, else the member of CallableFunction or
 * NewableFunction when it has call or construct signatures, else the member of Object. *ok is false when the global
 * interface is not in the program. */
static TsSymbol *cPropertyOfObjectOrGlobal(TsType *o, BName name, bool *ok) {
  *ok = true;
  TsSymbol *p = cPropertyOf(o, name);
  if (p) return p;
  cResolveSignatures(o);
  int fi = o->ncall ? 1 : (o->nconstruct ? 2 : 0);
  if (fi) {
    TsSymbol *g = cGlobalMemberIface(fi);
    if (!g) { *ok = false; return NULL; }
    TsSymbol *m = g->members ? bTableGet(g->members, name) : NULL;
    if (m && (m->flags & SF_Value) && !(m->flags & SF_Signature)) return m;
  }
  TsSymbol *ob = cGlobalMemberIface(0);
  if (!ob) { *ok = false; return NULL; }
  TsSymbol *m = ob->members ? bTableGet(ob->members, name) : NULL;
  return (m && (m->flags & SF_Value) && !(m->flags & SF_Signature)) ? m : NULL;
}

static TsType *cNonNullableType(TsType *t);

/* the object of a single-link optional chain e.expression?.name or e.expression?.[k] (checkPropertyAccessChain): a nullable
 * object gives its non-nullable type to the access (*nullable), and the access result has undefined added. A chain that
 * continues past this link (its parent accesses or calls it) is not ported; so is an object that is not a plain union */
static TsType *cOptionalObject(TsNode *e, TsType *o0, bool *nullable) {
  *nullable = false;
  if (o0 == cErrorType || (o0->flags & TF_Any)) return o0;
  TsNode *p = e->parent;
  if (p && (p->kind == TSK_PropertyAccessExpression || p->kind == TSK_ElementAccessExpression || p->kind == TSK_CallExpression) && p->expression == e)
    return cUnported("optional chain continuation");
  if (o0->flags & TF_Object) return o0;
  if (!(o0->flags & TF_Union)) return cUnported("optional chain of a non-object");
  bool nul = false;
  for (int i = 0; i < o0->ntypes; i++) {
    uint32_t f = o0->types[i]->flags;
    if (f & (TF_Undefined | TF_Null)) nul = true;
    else if (!(f & TF_Object)) return cUnported("optional chain of an unported union");
  }
  if (!nul) return o0;
  *nullable = true;
  return cNonNullableType(o0);
}

/* undefined added to the result of an optional access whose object was nullable (propagateOptionalTypeMarker) */
static TsType *cOptionalResult(TsType *r, bool nullable) {
  if (!nullable || r == cErrorType) return r;
  TsType *ts[2] = { r, cUndefinedType };
  return cGetUnionType(ts, 2);
}

/* the type of a member access e.expression.name (checkPropertyAccessExpression) */
static TsType *cPropertyAccessOf(TsNode *e, TsType *o) {
  if (o == cErrorType) return o;
  if (o->flags & TF_Any) return o;
  if (!e->name || e->name->kind != TSK_Identifier) return cUnported("private name access");
  o = cApparentOf(o);
  if (o == cErrorType) return o;
  BName pname = bnEscapeText(e->name->text, e->name->textLen);
  TsSymbol *p;
  if (o->flags & TF_Union) {
    p = cUnionProperty(o, pname);
    if (!p) return cUnported("property of a union");
  } else {
    if (!(o->flags & TF_Object)) return cUnported("property access on a non-object");
    bool gok;
    p = cPropertyOfObjectOrGlobal(o, pname, &gok);
    if (!gok) return cUnported("property access: no global object member");
    if (!p) return cUnported("property access: no property");
  }
  TsType *declared = cMemberType(o, p, e);
  if (declared == cErrorType) return declared;
  return cAccessReferenceFlow(e, declared, true);
}

static TsType *cPropertyAccessType(TsNode *e) {
  TsType *o0 = cExprType(e->expression);
  if (!e->questionDotToken) return cPropertyAccessOf(e, o0);
  bool nullable = false;
  TsType *o = cOptionalObject(e, o0, &nullable);
  if (o == cErrorType) return o;
  return cOptionalResult(cPropertyAccessOf(e, o), nullable);
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

static TsType *cElementAccessOf(TsNode *e, TsType *o);
static TsType *cElementAccessType(TsNode *e) {
  TsType *o0 = cExprType(e->expression);
  if (!e->questionDotToken) return cElementAccessOf(e, o0);
  bool nullable = false;
  TsType *o = cOptionalObject(e, o0, &nullable);
  if (o == cErrorType) return o;
  return cOptionalResult(cElementAccessOf(e, o), nullable);
}

static TsType *cElementAccessOf(TsNode *e, TsType *o) {
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
  if (kt->flags & TF_Any) {
    /* getApplicableIndexInfo of any: any is assignable to the string and the number key types, so the one index signature applies */
    cResolveIndexInfos(o);
    if (o->nindex == 1) return o->index[0].type;
    return cUnported("element access with an any key");
  }
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
    BName pname = bnEscapeText(l->name->text, l->name->textLen);
    TsSymbol *p;
    if (o->flags & TF_Union) {
      /* the write type of a union property is the union of the members' write types, which are their read types here */
      p = cUnionProperty(o, pname);
      if (!p) return cUnported("assignment to a member of a union");
      return cTypeOfSymbol(p);
    }
    if (!(o->flags & TF_Object)) return cUnported("assignment to a member of a non-object");
    p = cPropertyOf(o, pname);
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
 * this or rest parameter, no default type parameter, no const modifier, and constraints that are not primitive. A constraint
 * that mentions other type parameters is instantiated with the inferences of those parameters (cInferredOne). Candidates keep the candidate order of chooseOverload; the inference of each
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
  cSigInstantiate(&e->sig, base, e->sig.mapper);   /* getSignatureInstantiation: instantiateSignature(signature, mapper, true) */
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
  if (t->flags & TF_TypeParameter) return false;   /* another type parameter */
  if ((t->flags & TF_Object) && (t->objectFlags & OF_Anonymous) && t->symbol && (t->symbol->flags & SF_TypeLiteral) &&
      t->typeNode && t->typeNode->kind == TSK_TypeLiteral) {
    /* a type literal: its properties and index signatures (its call and construct signatures are not ported) */
    cResolveSignatures(t);
    if (t->ncall || t->nconstruct) { *unp = true; return false; }
    TsSymbol **props;
    int np = cPropertiesOf(t, &props);
    for (int k = 0; k < np; k++) {
      if (cMentionsTypeParam(cTypeOfSymbol(props[k]), tp, unp)) return true;
      if (*unp) return false;
    }
    cResolveIndexInfos(t);
    for (int k = 0; k < t->nindex; k++) {
      if (cMentionsTypeParam(t->index[k].keyType, tp, unp) || cMentionsTypeParam(t->index[k].type, tp, unp)) return true;
      if (*unp) return false;
    }
    return false;
  }
  if (t->flags & (TF_Union | TF_Intersection)) {
    for (int i = 0; i < t->ntypes; i++) if (cMentionsTypeParam(t->types[i], tp, unp)) return true;
    return false;
  }
  if ((t->flags & TF_Object) && (t->objectFlags & OF_Reference)) {
    for (int i = 0; i < t->nargs; i++) if (cMentionsTypeParam(t->args[i], tp, unp)) return true;
    return false;
  }
  if ((t->flags & TF_Object) && (t->objectFlags & OF_Anonymous) && t->typeNode && t->typeNode->kind == TSK_FunctionType && !t->target) {
    /* a function type literal: the parameters and the return type of its one signature */
    cResolveSignatures(t);
    if (t->ncall != 1 || t->nconstruct != 0) { *unp = true; return false; }
    CSig *s = &t->call[0];
    CParams cp;
    if ((s->decl->typeParameters && s->decl->typeParameters->n) || !cSplitParams(s->decl, &cp) || cp.thisParam) { *unp = true; return false; }
    for (int k = 0; k < cp.n; k++) {
      bool ok;
      TsType *x = cSigParamType(s, &cp, k, &ok);
      if (!ok) continue;
      if (x == cErrorType) { *unp = true; return false; }
      if (cMentionsTypeParam(x, tp, unp)) return true;
    }
    TsType *r = cReturnTypeOf(s);
    if (r == cErrorType) { *unp = true; return false; }
    return cMentionsTypeParam(r, tp, unp);
  }
  *unp = true;
  return false;
}

/* the contextual type of a function argument of a generic call (checkFunctionExpressionOrObjectLiteralMethod, inferential mode).
 * The contextual signature is the one of the parameter type. Inside the call being inferred (its inference context is found
 * with getInferenceContext), the parameter type is instantiated with the context's mapper (cInferFixContext: the type
 * parameters it mentions are fixed, and later inferences to them are ignored). Outside it, the parameter type is the
 * contextual type as it is declared, whose type parameters stay (contextuallyCheckFunctionExpressionOrObjectLiteralMethod
 * uses the contextual signature when there is no inference context). Several or generic signatures are not ported. */
static TsType *cGenericCallFnArgContext(CSig *sig, int idx, TsNode *arg) {
  CParams ps;
  if (!cSplitParams(sig->decl, &ps) || ps.thisParam || ps.rest) return cUnported("contextual type of a generic call");
  bool ok;
  TsType *pt = cSigParamType(sig, &ps, idx, &ok);
  if (!ok) return cUnported("contextual type of a generic call");
  if (pt == cErrorType) return pt;
  if (!(pt->flags & TF_Object)) return cUnported("contextual type of a generic call");
  cResolveSignatures(pt);
  if (pt->ncall != 1 || pt->nconstruct != 0) return cUnported("contextual type of a generic call");
  CSig *cs = &pt->call[0];
  CParams cp;
  if ((cs->decl->typeParameters && cs->decl->typeParameters->n) || !cSplitParams(cs->decl, &cp) || cp.thisParam)
    return cUnported("contextual type of a generic call");
  struct CCallInfer *ic = cInferenceContextOf(arg);
  if (!ic) return pt;
  TsType *pi = cInferFixContext(ic, pt);
  return pi ? pi : cErrorType;
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
/* an arrow with an unannotated parameter: its type depends on the contextual parameter types (isContextSensitive) */
static bool cIsContextSensitiveArg(TsNode *a) {
  a = cSkipParens(a);
  if (a->kind != TSK_ArrowFunction || !a->parameters) return false;
  for (int i = 0; i < a->parameters->n; i++) if (!a->parameters->v[i]->type) return true;
  return false;
}

static TsType *cExprTypeWithContext(TsNode *arg, TsType *ptype);

/* the inference context of one generic candidate (createInferenceContext and inferTypeArguments): the candidates of each type
 * parameter, the return type candidates, and the type parameters fixed by a contextual parameter type (getInferredType keeps
 * the type of a fixed parameter; later candidates do not change it) */
typedef struct CCallInfer {
  TsNode *call; CSig *sig; CParams ps; TsType **tps; int n; bool *prim; CInfer ci; bool *notTop; bool *fixed; TsType **fixedT;
  bool *busy;   /* getInferredType in progress for the parameter (a constraint that refers to another parameter) */
  int before;   /* cUnportedCount at the start: a refusal inside the inference ends it */
} CCallInfer;

static bool cInferBegin(CCallInfer *ic, TsNode *call, CSig *sig) {
  int n;
  TsType **tps = cSigTypeParams(sig->decl, &n);
  ic->call = call; ic->sig = sig; ic->tps = tps; ic->n = n;
  ic->before = cUnportedCount;
  if (!cSplitParams(sig->decl, &ic->ps)) { cUnported("too many parameters"); return false; }
  if (ic->ps.thisParam || ic->ps.rest) { cUnported("generic call with a this or rest parameter"); return false; }
  ic->prim = ts_arena_zalloc(sizeof(bool) * (size_t)(n ? n : 1));   /* hasPrimitiveConstraint */
  for (int k = 0; k < n; k++) {
    if (cTypeParamIsConst(tps[k])) { cUnported("generic call with a const type parameter"); return false; }
    TsType *c = cConstraintOf(tps[k]);
    if (c) {
      bool unp = false;
      ic->prim[k] = cMaybePrimitive(c, &unp);
      if (unp) { cUnported("generic call with a primitive constraint"); return false; }
    }
  }
  cInferInit(&ic->ci, tps, n);
  /* the contextual type of the call infers from the return type at the ReturnType priority (inferTypeArguments). Only the
   * arguments' candidates decide a type parameter that has them; one with only these candidates is unported below */
  TsType *ctx = cContextualType(call);
  if (ctx == cErrorType) return false;
  if (ctx) {
    TsType *ret0 = cReturnTypeOf(sig);
    if (ret0 == cErrorType) return false;
    if (cCouldContain(ret0)) {
      ic->ci.priority = 128;   /* InferencePriority.ReturnType */
      cInferFrom(&ic->ci, ctx, ret0);
      ic->ci.priority = 0;
    }
  }
  ic->notTop = ts_arena_zalloc(sizeof(bool) * (size_t)(n ? n : 1));
  ic->fixed = ts_arena_zalloc(sizeof(bool) * (size_t)(n ? n : 1));
  ic->fixedT = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  ic->busy = ts_arena_zalloc(sizeof(bool) * (size_t)(n ? n : 1));
  return cUnportedCount == ic->before;
}

/* inferTypes of one argument type against its parameter type. The type parameters fixed by then take no candidates (skipFixed) */
static bool cInferAddArg(CCallInfer *ic, TsType *argT, TsType *pt, bool skipFixed) {
  if (argT == cErrorType || pt == cErrorType) { cUnported("generic call with an unported argument or parameter"); return false; }
  if (!cCouldContain(pt)) return true;
  bool unp = false;
  for (int k = 0; k < ic->n; k++)
    if (cMentionsTypeParam(pt, ic->tps[k], &unp) && !cTypeParamAtTop(pt, ic->tps[k], &unp)) ic->notTop[k] = true;
  if (unp) { cUnported("generic call with a parameter of an unported shape"); return false; }
  /* checkExpressionWithContextualType keeps a literal argument regular when its parameter has a literal of its kind */
  if ((argT->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BooleanLiteral)) && !(pt->flags & TF_TypeParameter) &&
      cLiteralOfContext(argT, pt)) { cUnported("literal argument of a literal parameter type"); return false; }
  CInfer *c = &ic->ci;
  int *scn = ts_arena_alloc(sizeof(int) * (size_t)(ic->n ? ic->n : 1)), *sctn = ts_arena_alloc(sizeof(int) * (size_t)(ic->n ? ic->n : 1));
  int *sprio = ts_arena_alloc(sizeof(int) * (size_t)(ic->n ? ic->n : 1));
  for (int k = 0; k < ic->n; k++) { scn[k] = c->cn[k]; sctn[k] = c->ctn[k]; sprio[k] = c->prio[k]; }
  cInferFrom(c, argT, pt);
  if (skipFixed) for (int k = 0; k < ic->n; k++) if (ic->fixed[k]) { c->cn[k] = scn[k]; c->ctn[k] = sctn[k]; c->prio[k] = sprio[k]; }
  return cUnportedCount == ic->before;
}

/* getCovariantInference and getInferredType of one type parameter from its candidates (no fixing). NULL when not ported */
static TsType *cInferredOneCore(CCallInfer *ic, int k);

/* getUnionType(types, UnionReduction.Subtype) of several types: any, unknown and error decide the result, never is
 * dropped, the members are sorted by id and deduplicated, then the subtypes are removed */
static TsType *cSubtypeUnionOfList(TsType **ts, int n) {
  int cap = 1;
  for (int i = 0; i < n; i++) cap += cMemberCount(ts[i]);
  TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)cap);
  int m = 0;
  for (int i = 0; i < n; i++) {
    TsType *t = ts[i];
    if (t == cErrorType) return cErrorType;
    if (t->flags & TF_Any) return cAnyType;
    if (t->flags & TF_Unknown) return cUnknownType;
    for (int j = 0; j < cMemberCount(t); j++) if (!(cMemberAt(t, j)->flags & TF_Never)) v[m++] = cMemberAt(t, j);
  }
  for (int i = 1; i < m; i++) {
    TsType *x = v[i];
    int j = i - 1;
    while (j >= 0 && v[j]->id > x->id) { v[j + 1] = v[j]; j--; }
    v[j + 1] = x;
  }
  int d = 0;
  for (int i = 0; i < m; i++) if (d == 0 || v[d - 1] != v[i]) v[d++] = v[i];
  return d == 0 ? cNeverType : cUnionWithSubtypesRemoved(v, d);
}

/* getInferredType of parameter k of the context (without fixing it). A constraint that refers to another parameter is
 * instantiated through the non-fixing mapper (makeNonFixingMapperForContext) of the parameters it mentions; a cycle
 * through the constraints is not ported */
static TsType *cInferredOne(CCallInfer *ic, int k) {
  if (ic->busy[k]) { cUnported("generic call with a cyclic constraint"); return NULL; }
  ic->busy[k] = true;
  TsType *r = cInferredOneCore(ic, k);
  ic->busy[k] = false;
  return r;
}

static TsType *cInferredOneCore(CCallInfer *ic, int k) {
  CInfer *ci = &ic->ci;
  TsType *tp = ic->tps[k];
  /* PriorityImpliesCombination: the candidates are combined by a subtype union, not a common supertype */
  bool combine = ci->prio[k] >= 0 && (ci->prio[k] & 416);
  if (ci->ctn[k]) { cUnported("contravariant candidates of a generic call"); return NULL; }
  TsType *c = cConstraintOf(tp);
  TsType *inferred;
  if (ci->cn[k] == 0) {
    /* no candidate: the default (not ported when it mentions a type parameter), else the constraint, else unknown */
    TsNode *dn = cTypeParamDefaultNode(tp);
    if (dn) {
      inferred = cTypeFromNode(dn);
      if (inferred == cErrorType) return NULL;
      if (cCouldContain(inferred)) { cUnported("generic call with a default mentioning a type parameter"); return NULL; }
    } else {
      inferred = c ? c : cUnknownType;
    }
  } else {
    TsType **cs = ci->cand[k];
    int nc = ci->cn[k];
    for (int i = 0; i < nc; i++) {
      TsType *t = cs[i];
      if (t->flags & TF_Union) {
        for (int m = 0; m < t->ntypes; m++) if (t->types[m]->flags & TF_Object && (t->types[m]->objectFlags & (OF_ObjectLiteral | OF_ArrayLiteral))) { cUnported("object or array literal candidate"); return NULL; }
      } else if ((t->flags & TF_Object) && (t->objectFlags & (OF_ObjectLiteral | OF_ArrayLiteral))) {
        cUnported("object or array literal candidate");
        return NULL;
      }
    }
    /* getCovariantInference: a primitive constraint keeps the literal types (regular), else they widen at the top level */
    TsType *ret = cReturnTypeOf(ic->sig);
    if (ret == cErrorType) return NULL;
    if (ic->sig->decl->type && ic->sig->decl->type->kind == TSK_TypePredicate) { cUnported("generic call with a type predicate"); return NULL; }
    bool unp = false;
    bool atTop = cTypeParamAtTop(ret, tp, &unp);
    if (unp) { cUnported("generic call with a conditional return type"); return NULL; }
    bool widen = !ic->prim[k] && !ic->notTop[k] && !atTop;
    TsType **base = ts_arena_alloc(sizeof(TsType *) * (size_t)nc);
    for (int i = 0; i < nc; i++) base[i] = ic->prim[k] ? cRegularType(cs[i]) : widen ? cWidenLiteralType(cs[i]) : cs[i];
    for (int i = 0; i < nc; i++) if (base[i] == cErrorType) return NULL;
    inferred = combine ? cSubtypeUnionOfList(base, nc) : cCommonSupertype(base, nc);
    if (inferred == cErrorType) return NULL;
  }
  if (c) {
    TsType *inst = c;
    if (cCouldContain(c)) {
      bool unp = false;
      bool self = cMentionsTypeParam(c, ic->tps[k], &unp);
      if (unp) { cUnported("generic call with a constraint of an unported shape"); return NULL; }
      if (self) { cUnported("generic call with a constraint that refers to its own parameter"); return NULL; }
      /* instantiateType(constraint, context.nonFixingMapper): only the parameters the constraint mentions are evaluated */
      int m = 0;
      TsType **src = ts_arena_alloc(sizeof(TsType *) * (size_t)(ic->n ? ic->n : 1)), **tgt = ts_arena_alloc(sizeof(TsType *) * (size_t)(ic->n ? ic->n : 1));
      for (int j = 0; j < ic->n; j++) {
        if (!cMentionsTypeParam(c, ic->tps[j], &unp)) continue;
        if (unp) { cUnported("generic call with a constraint of an unported shape"); return NULL; }
        TsType *v = ic->fixed[j] ? ic->fixedT[j] : cInferredOne(ic, j);
        if (!v) return NULL;
        src[m] = ic->tps[j];
        tgt[m] = v;
        m++;
      }
      inst = cInstantiate(c, cCreateTypeMapper(src, tgt, m));
      if (inst == cErrorType) return NULL;
    }
    if (!cIsRelatedWith(inferred, inst, REL_Assignable)) inferred = inst;
  }
  return inferred;
}

/* the type arguments of the candidate: the fixed type parameters keep their types. NULL when not ported */
static bool cInferEnd(CCallInfer *ic, TsType **args) {
  for (int k = 0; k < ic->n; k++) {
    TsType *v = ic->fixed[k] ? ic->fixedT[k] : cInferredOne(ic, k);
    if (!v) return false;
    args[k] = v;
  }
  return cUnportedCount == ic->before;
}

/* the contextual type of a context-sensitive argument (the parameter type pt): the type parameters that the parameters of its
 * signature mention are fixed (assignContextualParameterTypes), the others are instantiated with their current inferences.
 * NULL when not ported */
static TsType *cInferFixContext(CCallInfer *ic, TsType *pt) {
  int n = ic->n;
  bool unp = false;
  bool *mention = ts_arena_zalloc(sizeof(bool) * (size_t)(n ? n : 1));
  bool *inParams = ts_arena_zalloc(sizeof(bool) * (size_t)(n ? n : 1));
  for (int k = 0; k < n; k++) mention[k] = cMentionsTypeParam(pt, ic->tps[k], &unp);
  if (unp) { cUnported("contextual type of a generic call with an unported shape"); return NULL; }
  bool single = false;
  if (pt->flags & TF_Object) { cResolveSignatures(pt); single = pt->ncall == 1; }
  if (single) {
    CSig *cs = &pt->call[0];
    CParams cp;
    if (!cSplitParams(cs->decl, &cp)) { cUnported("too many parameters"); return NULL; }
    for (int j = 0; j < cp.n; j++) {
      bool ok;
      TsType *t = cSigParamType(cs, &cp, j, &ok);
      if (!ok || t == cErrorType) { cUnported("contextual parameter of an unported shape"); return NULL; }
      for (int k = 0; k < n; k++) if (cMentionsTypeParam(t, ic->tps[k], &unp)) inParams[k] = true;
    }
    if (unp) { cUnported("contextual parameter of an unported shape"); return NULL; }
  }
  for (int k = 0; k < n; k++) {
    if (!mention[k]) continue;
    if (!single || inParams[k]) {
      if (!ic->fixed[k]) {
        TsType *v = cInferredOne(ic, k);
        if (!v) return NULL;
        ic->fixed[k] = true;
        ic->fixedT[k] = v;
      }
    }
  }
  TsType **src = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1)), **dst = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  int cnt = 0;
  for (int k = 0; k < n; k++) {
    if (!mention[k]) continue;
    TsType *v = ic->fixed[k] ? ic->fixedT[k] : cInferredOne(ic, k);
    if (!v) return NULL;
    src[cnt] = ic->tps[k];
    dst[cnt] = v;
    cnt++;
  }
  if (cnt == 0) return pt;
  TsType *r = cInstantiate(pt, cCreateTypeMapper(src, dst, cnt));
  return r == cErrorType ? NULL : r;
}

/* the inference contexts of the generic candidates whose arguments are being checked (the innermost last). A call's own
 * inference context is the innermost one of the call whose argument contains the node (getInferenceContext): the node
 * is then checked in the inferential mode of its call. */
#define C_INFER_STACK_MAX 256
static struct CCallInfer *cInferStack[C_INFER_STACK_MAX];
static int cInferDepth;

/* getInferenceContext(node): the innermost inference context whose call has the node inside one of its arguments */
static struct CCallInfer *cInferenceContextOf(TsNode *node) {
  if (cInferDepth == 0) return NULL;
  struct CCallInfer *ic = cInferStack[cInferDepth - 1];
  TsNode *child = node;
  TsNode *p = node->parent;
  while (p && p != ic->call) { child = p; p = p->parent; }
  if (!p || !ic->call->arguments) return NULL;
  for (int i = 0; i < ic->call->arguments->n; i++) if (ic->call->arguments->v[i] == child) return ic;
  return NULL;
}

/* the signature instance of a generic candidate after its inference (inferTypeArguments in round 1, with the context-sensitive
 * arguments skipped, or in round 2, with them typed by their contextual types in argument order). NULL with *err when not ported */
static CSig *cGenericCandidate(CSig *base, CParams *ps, TsNode *call, TsType **at, bool *deferred, TsNode **args, int na,
                               bool typed, bool *err) {
  CCallInfer ic;
  CSig *r = NULL;
  if (!cInferBegin(&ic, call, base)) { *err = true; return NULL; }
  if (cInferDepth == C_INFER_STACK_MAX) { cUnported("generic call nested too deeply"); *err = true; return NULL; }
  cInferStack[cInferDepth++] = &ic;
  for (int i = 0; i < na; i++) {
    bool ok;
    TsType *pt = cSigParamType(base, ps, i, &ok);
    if (!ok || pt == cErrorType) { *err = true; goto done; }
    if (!deferred[i]) {
      if (!cInferAddArg(&ic, at[i], pt, false)) { *err = true; goto done; }
      continue;
    }
    if (!typed) continue;
    TsType *pi = cInferFixContext(&ic, pt);
    if (!pi) { *err = true; goto done; }
    TsType *argT = cExprTypeWithContext(args[i], pi);
    if (!cInferAddArg(&ic, argT, pt, true)) { *err = true; goto done; }
  }
  TsType **targs = ts_arena_alloc(sizeof(TsType *) * (size_t)(ic.n ? ic.n : 1));
  if (!cInferEnd(&ic, targs)) { *err = true; goto done; }
  r = cGetSignatureInstance(base, targs, ic.n);
done:
  cInferDepth--;
  return r;
}

/* inferTypeArguments of a generic candidate of a call whose arguments are all typed (no deferred argument) */
static bool cInferCallTypeArgs(TsNode *call, CSig *sig, TsType **at, int na, TsType **args) {
  CCallInfer ic;
  if (!cInferBegin(&ic, call, sig)) return false;
  /* the arguments that are not context sensitive first, then the others (the second pass of resolveCall) */
  int *ord = ts_arena_alloc(sizeof(int) * (size_t)(na ? na : 1));
  int nord = 0;
  for (int pass = 0; pass < 2; pass++)
    for (int i = 0; i < na; i++)
      if (cIsContextSensitiveArg(call->arguments->v[i]) == (pass == 1)) ord[nord++] = i;
  for (int oi = 0; oi < na; oi++) {
    int i = ord[oi];
    bool ok;
    TsType *pt = cSigParamType(sig, &ic.ps, i, &ok);
    if (!ok || pt == cErrorType || at[i] == cErrorType) { cUnported("generic call with an unported argument or parameter"); return false; }
    if (!cInferAddArg(&ic, at[i], pt, false)) return false;
  }
  return cInferEnd(&ic, args);
}

/* isContextSensitive of a function expression or arrow (hasContextSensitiveParameters: a parameter without a type, and no
 * type parameters) */
static bool cIsContextSensitiveFn(TsNode *a) {
  TsNode *f = cSkipParens(a);
  if (!f || (f->kind != TSK_ArrowFunction && f->kind != TSK_FunctionExpression)) return false;
  if (f->typeParameters && f->typeParameters->n) return false;
  if (!f->parameters) return false;
  for (int i = 0; i < f->parameters->n; i++) if (!f->parameters->v[i]->type) return true;
  return false;
}

/* the type of a function argument checked with the parameter type of a candidate (the contextual type of the argument) */
static TsType *cExprTypeWithContext(TsNode *arg, TsType *ptype) {
  TsNode *saveFn = cCandFn;
  TsType *saveType = cCandType;
  cCandFn = cSkipParens(arg);
  cCandType = ptype;
  TsType *t = cExprType(arg);
  cCandFn = saveFn;
  cCandType = saveType;
  return t;
}

/* an argument whose type can depend on the contextual type, which is the parameter type of the candidate in an overloaded
 * call (getContextualType of a call argument with several signatures): everything but the expressions that never do (an
 * element access and a non-null assertion take their type from their operands) */
static bool cArgNeedsContext(TsNode *a) {
  TsNode *c = cSkipParens(a);
  switch (c->kind) {
    case TSK_Identifier: case TSK_NumericLiteral: case TSK_StringLiteral: case TSK_NoSubstitutionTemplateLiteral:
    case TSK_NullKeyword: case TSK_TrueKeyword: case TSK_FalseKeyword: case TSK_PropertyAccessExpression:
    case TSK_ElementAccessExpression: case TSK_NonNullExpression:
      return false;
    default: return true;
  }
}

/* getSignatureApplicabilityError of one candidate for chooseOverload: the arguments are related in order and the first
 * failure ends it. A deferred argument is checked with the parameter type of the candidate (checkExpressionWithContextualType);
 * in skip mode a context-sensitive function is anyFunctionType. Returns 1 when applicable, 0 when not, -1 when the
 * parameter type is not ported. */
static int cCandidateApplicable(CSig *sig, CParams *ps, TsType **at, bool *deferred, TsNode **args, int na, int rel, bool skipMode) {
  for (int i = 0; i < na; i++) {
    TsType *a;
    if (!deferred[i]) {
      a = at[i];
    } else {
      bool ok;
      TsType *pt = cSigParamType(sig, ps, i, &ok);
      if (!ok || pt == cErrorType) return -1;
      a = skipMode && cIsContextSensitiveFn(args[i]) ? cAnyFunctionType : cExprTypeWithContext(args[i], pt);
    }
    if (a == cErrorType) continue;
    bool ok;
    TsType *pt = cSigParamType(sig, ps, i, &ok);
    if (!ok || pt == cErrorType) continue;
    if (!cIsRelatedWith(a, pt, rel)) return 0;
  }
  return 1;
}

/* chooseOverload of a call with deferred arguments (5.9.2). argCheckMode is shared by the candidates and by both relation
 * passes of resolveCall: it stays SkipContextSensitive until a candidate passes the skip check, then it is Normal for the rest
 * of the call. A generic candidate is inferred in round 1 (its context-sensitive arguments skipped) in skip mode, and in round 2
 * (them typed) in normal mode. Returns NULL when no candidate is applicable; *err is set when a candidate is not ported. */
static CSig *cChooseDeferredOverload(CSig *sigs, CParams *pss, bool *arityOk, bool *generic, int ns, TsType **at, bool *deferred,
                                     TsNode *call, TsNode **args, int na, bool *err) {
  bool skipMode = true;
  for (int pass = ns > 1 ? 0 : 1; pass < 2; pass++) {   /* a single candidate is checked with the assignable relation only */
    int rel = pass == 0 ? REL_Subtype : REL_Assignable;
    for (int c = 0; c < ns; c++) {
      if (!arityOk[c]) continue;
      CSig *sig = &sigs[c];
      if (generic[c]) {
        /* only the context-sensitive functions are inferred in rounds (the other deferred arguments are not ported) */
        for (int i = 0; i < na; i++) if (deferred[i] && !cIsContextSensitiveFn(args[i])) { *err = true; return NULL; }
        if (skipMode) {
          sig = cGenericCandidate(&sigs[c], &pss[c], call, at, deferred, args, na, false, err);
          if (!sig) return NULL;
        }
      }
      if (skipMode) {
        int r = cCandidateApplicable(sig, &pss[c], at, deferred, args, na, rel, true);
        if (r < 0) { *err = true; return NULL; }
        if (r == 0) continue;
        skipMode = false;
      }
      if (generic[c]) {
        sig = cGenericCandidate(&sigs[c], &pss[c], call, at, deferred, args, na, true, err);
        if (!sig) return NULL;
      }
      int r = cCandidateApplicable(sig, &pss[c], at, deferred, args, na, rel, false);
      if (r < 0) { *err = true; return NULL; }
      if (r == 1) return sig;
    }
  }
  return NULL;
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
  CParams *pss = ts_arena_alloc(sizeof(CParams) * (size_t)ns);
  bool *arityOk = ts_arena_alloc(sizeof(bool) * (size_t)ns);
  bool *generic = ts_arena_zalloc(sizeof(bool) * (size_t)ns);
  for (int c = 0; c < ns; c++) {
    CSig *sig = &sigs[c];
    generic[c] = sig->decl->typeParameters && sig->decl->typeParameters->n > 0;
    if (!cSplitParams(sig->decl, &pss[c])) return cUnported("too many parameters");
    if (pss[c].thisParam) return cUnported("call of a signature with a this parameter");
    if (generic[c] && pss[c].rest) return cUnported("call of a generic signature with a rest parameter");
    int mx = pss[c].rest ? 1 << 30 : pss[c].n;
    arityOk[c] = na >= pss[c].min && na <= mx;
  }
  TsType **at = ts_arena_alloc(sizeof(TsType *) * (size_t)(na ? na : 1));
  /* the context-sensitive function arguments of several candidates (or of one generic candidate) are typed per candidate
   * (chooseOverload); the other arguments that depend on their contextual type are typed per candidate too when there are several */
  bool *deferred = ts_arena_zalloc(sizeof(bool) * (size_t)(na ? na : 1));
  bool anyDeferred = false;
  if (ns > 1) for (int i = 0; i < na; i++) if (cArgNeedsContext(e->arguments->v[i])) { deferred[i] = true; anyDeferred = true; }
  if (ns == 1 && generic[0]) for (int i = 0; i < na; i++) if (cIsContextSensitiveFn(e->arguments->v[i])) { deferred[i] = true; anyDeferred = true; }
  for (int i = 0; i < na; i++) if (!deferred[i]) at[i] = cExprType(e->arguments->v[i]);
  /* the return type inference from a contextual type is not ported */
  /* the candidate that is applicable first (in declaration order, among those of the right arity) */
  int chosen = -1;
  CSig *chosenSig = NULL;
  if (anyDeferred) {
    bool err = false;
    chosenSig = cChooseDeferredOverload(sigs, pss, arityOk, generic, ns, at, deferred, e, e->arguments->v, na, &err);
    if (err) return cUnported("overloaded call with a function argument reaches a generic candidate or an unported parameter");
    if (!chosenSig) {
      /* no candidate: the arguments are typed without a context (unported) so that their bodies are still checked */
      for (int i = 0; i < na; i++) if (deferred[i]) cExprType(e->arguments->v[i]);
      return cUnported("no applicable signature");
    }
    chosen = 0;
  }
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
      if (!cInferCallTypeArgs(e, &sigs[c], at, na, targs)) return cErrorType;
      CSig *inst = cGetSignatureInstance(&sigs[c], targs, ntp);
      if (cArgsApplicable(&pss[c], at, na, inst, rel)) { chosen = c; chosenSig = inst; }
    }
  }
  if (chosen < 0) return cUnported("no applicable signature");
  e->resolvedSig = chosenSig;   /* links.resolvedSignature (getContextualTypeForArgumentAtIndex reads it) */
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

/* createArrayLiteralType: the ArrayLiteral clone of a reference (cloneTypeReference), one per reference */
static TsType *cArrayLiteralOf(TsType *ref) {
  if (!ref->literalType) {
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

/* the element type of an array literal: the subtype-reduced union of the widened elements */
static TsType *cArrayLiteralType(TsNode *e) {
  int n = e->elements ? e->elements->n : 0;
  /* checkArrayLiteral: an empty literal has the element implicitNeverType under strictNullChecks, which the port takes as never
   * (the evolving array of an auto-typed variable is a different type and is not ported) */
  TsType *elem = cNeverType;
  if (n > 0) {
    TsType **ts = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
    for (int i = 0; i < n; i++) {
      TsNode *el = e->elements->v[i];
      if (el->kind == TSK_SpreadElement) return cUnported("spread in an array literal");
      if (el->kind == TSK_OmittedExpression) { ts[i] = cUndefinedType; continue; }
      TsType *t = cMutableLocationType(el, cExprType(el));
      if (t == cErrorType) return t;
      ts[i] = t;
    }
    elem = n == 1 ? ts[0] : cUnionWithSubtypesRemoved(ts, n);
  }
  if (elem == cErrorType) return elem;
  TsSymbol *arr = bTableGet(cGlobals, bnC("Array"));
  if (!arr) return cUnported("no global Array");
  TsType *target = cDeclaredTypeOfClassOrInterface(arr);
  return cArrayLiteralOf(cTypeReference(target, &elem, 1));
}

/* checkArrayLiteral with a tuple contextual type (createArrayLiteralType over createTupleType of the elements): each element is
 * widened for a mutable location, as in the array case. Spread and omitted elements are not ported. */
static TsType *cTupleArrayLiteralType(TsNode *e) {
  int n = e->elements ? e->elements->n : 0;
  if (n >= 16) return cUnported("tuple literal of 16 or more elements");   /* cTupleTarget caches the targets below 16 */
  TsType **ts = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  for (int i = 0; i < n; i++) {
    TsNode *el = e->elements->v[i];
    if (el->kind == TSK_SpreadElement || el->kind == TSK_OmittedExpression) return cUnported("spread or omitted element of a tuple literal");
    TsType *t = cMutableLocationType(el, cExprType(el));
    if (t == cErrorType) return t;
    ts[i] = t;
  }
  return cArrayLiteralOf(cTupleReference(ts, n, false));
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
      /* the result is boolean whatever the operands are (checkBinaryLikeExpression); the comparability errors are not ported,
       * so an unported operand does not change the result, only the operands' own checks (which are still made) */
      cExprType(e->left);
      cExprType(e->right);
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

/* ---- template expressions (checkTemplateExpression, 84988) ---- */
/* a span that node's evaluate (23423) may fold to a value: a literal, a template whose spans may all fold, a binary
 * operator of two such operands, a unary + - ~ of one, an entity name (a const variable, an enum member, Infinity, NaN)
 * or an access of a name. The other expressions give no value (evaluate gives undefined). */
static bool cSpanMayEvaluate(TsNode *e) {
  e = cSkipParens(e);
  switch (e->kind) {
    case TSK_NumericLiteral: case TSK_StringLiteral: case TSK_NoSubstitutionTemplateLiteral: return true;
    case TSK_TemplateExpression:
      for (int i = 0; e->spans && i < e->spans->n; i++) if (!cSpanMayEvaluate(e->spans->v[i]->expression)) return false;
      return true;
    case TSK_BinaryExpression: return cSpanMayEvaluate(e->left) && cSpanMayEvaluate(e->right);
    case TSK_PrefixUnaryExpression:
      return (e->extra == TSK_PlusToken || e->extra == TSK_MinusToken || e->extra == TSK_TildeToken) && cSpanMayEvaluate(e->operand);
    case TSK_PropertyAccessExpression: {
      TsNode *r = e;
      while (r->kind == TSK_PropertyAccessExpression) r = cSkipParens(r->expression);
      return r->kind == TSK_Identifier;
    }
    case TSK_ElementAccessExpression: return true;
    case TSK_Identifier: {
      if (e->textLen == 8 && memcmp(e->text, "Infinity", 8) == 0) return true;
      if (e->textLen == 3 && memcmp(e->text, "NaN", 3) == 0) return true;
      TsSymbol *s = cResolveValueName(e, bnEscapeText(e->text, e->textLen));
      if (!s) return false;
      if (s->flags & SF_EnumMember) return true;
      TsNode *d = cFirstDeclOf(s);
      return d && d->kind == TSK_VariableDeclaration && cIsConstVariableDecl(d);
    }
    default: return false;
  }
}

/* isTemplateLiteralContext: a parenthesized expression of one, or the argument of an element access */
static bool cIsTemplateLiteralCtx(TsNode *n) {
  TsNode *p = n->parent;
  if (p && p->kind == TSK_ParenthesizedExpression) return cIsTemplateLiteralCtx(p);
  return p && p->kind == TSK_ElementAccessExpression && p->argumentExpression == n;
}

/* isConstContext: 1 in a const context, 0 out of one, -1 when an assertion on the way up is not ported */
static int cIsConstContextState(TsNode *n) {
  TsNode *p = n->parent;
  if (!p) return 0;
  switch (p->kind) {
    case TSK_AsExpression: case TSK_TypeAssertionExpression: return -1;
    case TSK_ParenthesizedExpression: case TSK_ArrayLiteralExpression: case TSK_SpreadElement: return cIsConstContextState(p);
    case TSK_PropertyAssignment: case TSK_ShorthandPropertyAssignment: case TSK_TemplateSpan:
      return p->parent ? cIsConstContextState(p->parent) : 0;
    default: return 0;
  }
}

/* checkTemplateExpression: each span is checked in order and related to the template constraint (a span that is not is
 * string). The template literal type is kept in a const context, in a template literal context, or under a contextual type
 * with a literal or template member; otherwise the type is string. A template whose spans may all fold to a value is not
 * ported (evaluate), and neither is a span of a symbol type or a generic contextual type. */
static TsType *cTemplateExprType(TsNode *e) {
  int ns = e->spans ? e->spans->n : 0;
  CStr *texts = ts_arena_alloc(sizeof(CStr) * (size_t)(ns + 1));
  TsType **types = ts_arena_alloc(sizeof(TsType *) * (size_t)(ns ? ns : 1));
  texts[0].s = e->head->text;
  texts[0].n = e->head->textLen;
  bool err = false, allMaybe = ns > 0;
  TsType *tc = cGetUnionType((TsType *[]){cStringType, cNumberType, cBigIntType, cBooleanType, cNullType, cUndefinedType}, 6);
  for (int i = 0; i < ns; i++) {
    TsNode *sp = e->spans->v[i];
    TsType *t = cExprType(sp->expression);
    if (!cSpanMayEvaluate(sp->expression)) allMaybe = false;
    texts[i + 1].s = sp->literal->text;
    texts[i + 1].n = sp->literal->textLen;
    if (t == cErrorType) { err = true; types[i] = cErrorType; continue; }
    int nm = (t->flags & TF_Union) ? t->ntypes : 1;
    for (int k = 0; k < nm; k++) {
      TsType *m = (t->flags & TF_Union) ? t->types[k] : t;
      if (m->flags & (TF_ESSymbol | TF_UniqueESSymbol)) return cUnported("template of a symbol");
    }
    types[i] = cIsRelatedWith(t, tc, REL_Assignable) ? t : cStringType;
  }
  if (err) return cErrorType;
  for (int i = 0; i <= ns; i++) if (cHasEscapeOrNonAscii(texts[i].s, texts[i].n)) return cUnported("template with an escape");
  if (allMaybe) return cUnported("template that may fold to a value");
  bool lit = false;
  int cs = cIsConstContextState(e);
  if (cs < 0) return cUnported("template in an assertion");
  lit = cs > 0 || cIsTemplateLiteralCtx(e);
  if (!lit) {
    TsType *cx = cContextualType(e);
    if (cx == cErrorType) return cx;
    if (cx) {
      int n = (cx->flags & TF_Union) ? cx->ntypes : 1;
      for (int i = 0; i < n; i++) {
        TsType *m = (cx->flags & TF_Union) ? cx->types[i] : cx;
        if (m->flags & (TF_TypeParameter | TF_IndexedAccess | TF_Conditional)) return cUnported("template with a generic contextual type");
        if (m->flags & (TF_StringLiteral | TF_TemplateLiteral)) lit = true;
      }
    }
  }
  if (!lit) return cStringType;
  return cGetTemplateLiteralType(texts, types, ns);
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
    case TSK_TemplateExpression: return cTemplateExprType(e);
    case TSK_ArrayLiteralExpression: {
      TsType *ac = cContextualType(e);
      if (ac == cErrorType) return ac;
      if (ac != NULL && cIsTupleLikeContext(ac)) return cTupleArrayLiteralType(e);
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
        CCombinedSig *cn = cContextualSigNode(e, &err);
        if (err) return cUnported("function expression with a contextual type");
        if (!cn) {
          /* no contextual signature (an any context, or none applicable): the parameters take no contextual type, so they
           * must be annotated (an unannotated one would be implicitly any) */
          if (e->parameters) for (int i = 0; i < e->parameters->n; i++)
            if (!e->parameters->v[i]->type) return cUnported("function expression with a contextual type");
        } else {
          /* assignContextualParameterTypes: the parameters take their types in this check, while the contextual signature is the
           * one of the candidate (a request made later has no context, and a refusal would be cached on the symbol) */
          if (e->parameters) for (int i = 0; i < e->parameters->n; i++)
            if (!e->parameters->v[i]->type && e->parameters->v[i]->symbol) (void)cTypeOfSymbol(e->parameters->v[i]->symbol);
          if (!e->type) {
            TsType *cr = cCombinedReturn(cn);
            if (cr == cErrorType) return cr;
            if (cHasLiteralOrVariable(cr)) {
              TsType *rt = cInferReturnType(e);
              if (rt == cErrorType) return rt;
              if (e->retLiteral) return cUnported("literal return of a contextually typed function");
            }
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
