/*
 * Checker port, part 4: types, declared types of symbols and the identity relation (TypeScript
 * 5.9.2 checker.ts; line numbers in lib/typescript.js). Used by the checks that compare the types of
 * merged declarations (checkVariableLikeDeclaration, 87788).
 *
 * PORTED:
 *   - type objects (createType/createObjectType/createIntrinsicType; TypeFlags and ObjectFlags of
 *     TypeScript 5.9.2) with the checker's typeCount;
 *   - the types of type nodes that the immer-tiny declarations use: keyword types, literal types
 *     null/true/false/string and number literals, type references to interfaces and classes (with
 *     type arguments as references), to type aliases (followed) and to type parameters, union and
 *     intersection types (flattened, as getUnionType does), parenthesized types, array types, type
 *     literals, function and constructor types (anonymous object types), `typeof` of a global;
 *   - declared types of interfaces and classes (getDeclaredTypeOfClassOrInterface) with their
 *     resolved members: merged declarations, inherited members of non-generic base types;
 *   - getTypeOfSymbol for variables, properties (optional adds undefined), methods, functions,
 *     parameters (through signatures) and aliases of imports;
 *   - signatures (call, construct) and index infos from declarations;
 *   - the identity relation: isTypeIdenticalTo, propertiesIdenticalTo, signaturesIdenticalTo,
 *     compareSignaturesIdentical, indexSignaturesIdenticalTo, with the identity cache counted.
 * NOT PORTED (any of these makes the type the error type and sets cUnported; a comparison that
 * meets it is reported as "unported" and the validator fails): generic instantiation of members
 * (instantiateType with a mapper), tuple, mapped, conditional, indexed access, keyof, template and
 * literal-union types, this types, rest parameters in identity, type predicates, enum types, class
 * constructor types, accessors.
 */
#ifndef TS_TYPES_H
#define TS_TYPES_H

#include "ts-checker-decls.h"

/* TypeFlags (TypeScript 5.9.2) */
enum {
  TF_Any = 1, TF_Unknown = 2, TF_String = 4, TF_Number = 8, TF_Boolean = 16, TF_Enum = 32, TF_BigInt = 64,
  TF_StringLiteral = 128, TF_NumberLiteral = 256, TF_BooleanLiteral = 512, TF_EnumLiteral = 1024,
  TF_BigIntLiteral = 2048, TF_ESSymbol = 4096, TF_UniqueESSymbol = 8192, TF_Void = 16384, TF_Undefined = 32768,
  TF_Null = 65536, TF_Never = 131072, TF_TypeParameter = 262144, TF_Object = 524288, TF_Union = 1048576,
  TF_Intersection = 2097152, TF_NonPrimitive = 67108864
};

/* ObjectFlags (TypeScript 5.9.2) */
enum { OF_Class = 1, OF_Interface = 2, OF_Reference = 4, OF_Tuple = 8, OF_Anonymous = 16, OF_Instantiated = 64, OF_ObjectLiteral = 128 };

typedef struct CSig CSig;
typedef struct CIndexInfo { struct TsType *keyType, *type; bool isReadonly; } CIndexInfo;
/* an instantiation of a generic type by type arguments (the type's instantiations list) */
typedef struct TsInst { struct TsType **key; int n; struct TsType *val; } TsInst;

typedef struct TsType {
  uint32_t flags, objectFlags;
  int32_t id;
  TsSymbol *symbol;              /* the symbol of an object type, the alias of a union, the param of a type parameter */
  /* object types */
  TsSymTable *members;           /* resolved members (own and inherited); property, signature and index symbols */
  bool resolved;
  struct TsType **bases;         /* base types of an interface or class */
  int nbases;
  struct TsType *target;         /* references: the generic declared type; */
  struct TsType **args;          /* references: type arguments */
  int nargs;
  struct TsType **typeParams;    /* generic declared types: their type parameters */
  int ntypeParams;
  TsNode *typeNode;              /* anonymous types from a type node; literal value */
  struct TsMapper *mapper;       /* instantiated anonymous types: the mapper of the instantiation */
  struct TsType **outerTps;      /* anonymous types: the outer type parameters of the declaration */
  int nOuterTps;
  bool outerDone, instsInit, declDone, basesDone;
  TsInst *insts;                 /* instantiations of this generic type, by type arguments */
  int ninsts, capInsts;
  uint8_t couldContain;          /* couldContainTypeVariables: 0 not computed, 1 yes, 2 no */
  struct TsMapper *sigMapper;    /* the mapper of the signatures and index infos of this type */
  TsSymTable *declMembers;       /* declared members of an interface, class or type literal (own and late) */
  /* union and intersection */
  struct TsType **types;
  int ntypes;
  /* literals */
  const char *text;
  int textLen;
  /* resolved signatures and index infos */
  bool sigsResolved;
  CSig *call, *construct;
  int ncall, nconstruct;
  CIndexInfo *index;
  int nindex;
  /* relations (ts-relate.h): variances of a generic type, the constraint of a type parameter */
  uint8_t *variances;
  uint8_t varState;
  struct TsType *tpConstraint;
  uint8_t tpConstState;
} TsType;

struct CSig { TsNode *decl; bool isConstruct; TsType *thisType; struct TsMapper *mapper; };

/* createType counts (typeCount) */
static int cTypeCount;
static int cNextTypeId = 1;
static bool cUnportedHit;          /* set when a type or relation outside the ported subset was met */
static int cUnportedCount;
static int cIdentityCacheCount;

static TsType *cNewType(uint32_t flags) {
  cTypeCount++;
  TsType *t = ts_arena_zalloc(sizeof(TsType));
  t->flags = flags;
  t->id = cNextTypeId++;
  return t;
}

static TsType *cErrorType, *cAnyType, *cUnknownType, *cStringType, *cNumberType, *cBigIntType, *cBooleanType;
static TsType *cESSymbolType, *cVoidType, *cUndefinedType, *cNullType, *cNeverType, *cNonPrimitiveType;
static TsType *cTrueType, *cFalseType, *cGlobalThisType;
static TsType *cIntrinsicMarkerType;   /* intrinsicMarkerType: the type of `intrinsic` in lib type aliases */

/* the type of globalThis (createObjectType(Anonymous, globalThisSymbol), 16) */
static void cCreateGlobalThisType(void) {
  cGlobalThisType = cNewType(TF_Object);
  cGlobalThisType->objectFlags = OF_Anonymous;
  cGlobalThisType->symbol = cGlobalThisSymbol;
}

static TsType *cIntrinsic(uint32_t flags) { return cNewType(flags); }

static void cCreateIntrinsicTypes(void) {
  cTypeCount = 0;
  cNextTypeId = 1;
  cErrorType = cIntrinsic(TF_Any);
  cAnyType = cIntrinsic(TF_Any);
  cUnknownType = cIntrinsic(TF_Unknown);
  cStringType = cIntrinsic(TF_String);
  cNumberType = cIntrinsic(TF_Number);
  cBigIntType = cIntrinsic(TF_BigInt);
  cTrueType = cIntrinsic(TF_BooleanLiteral);
  cFalseType = cIntrinsic(TF_BooleanLiteral);
  cBooleanType = cIntrinsic(TF_Boolean | TF_Union);
  cBooleanType->types = ts_arena_alloc(sizeof(TsType *) * 2);
  cBooleanType->types[0] = cFalseType;
  cBooleanType->types[1] = cTrueType;
  cBooleanType->ntypes = 2;
  cESSymbolType = cIntrinsic(TF_ESSymbol);
  cVoidType = cIntrinsic(TF_Void);
  cUndefinedType = cIntrinsic(TF_Undefined);
  cNullType = cIntrinsic(TF_Null);
  cNeverType = cIntrinsic(TF_Never);
  cNonPrimitiveType = cIntrinsic(TF_NonPrimitive);
  cIntrinsicMarkerType = cIntrinsic(TF_Any);
}

/* ---- unported marker ---- */
static TsType *cUnported(const char *what) {
  cUnportedHit = true;
  cUnportedCount++;
#ifdef TS_TYPES_DEBUG
  fprintf(stderr, "UNPORTED %s\n", what);
#else
  (void)what;
#endif
  return cErrorType;
}

/* ---- declared types, symbols ---- */
static TsType *cTypeFromNode(TsNode *node);
static TsType *cTypeOfSymbol(TsSymbol *sym);
static bool cIsSignatureName(BName n);

/* the type of a type parameter symbol (createTypeParameter, cached on the symbol) */
static TsType *cTypeOfTypeParameter(TsSymbol *sym) {
  sym = cGetMergedSymbol(sym);
  if (sym->declaredType) return sym->declaredType;
  TsType *t = cNewType(TF_TypeParameter);
  t->symbol = sym;
  sym->declaredType = t;
  return t;
}

/* ts-instantiate.h: mappers, instantiation, type references (createTypeReference), unions and intersections */
#include "ts-instantiate.h"

/* literal types are cached per value (getStringLiteralType / getNumberLiteralType) */
static TsType **cLitCache;
static int cLitN, cLitCap;
static TsType *cLiteralType(uint32_t flags, const char *text, int n) {
  for (int i = 0; i < cLitN; i++) {
    TsType *l = cLitCache[i];
    if (l->flags == flags && l->textLen == n && memcmp(l->text, text, (size_t)n) == 0) return l;
  }
  TsType *l = cNewType(flags);
  l->text = text;
  l->textLen = n;
  if (cLitN == cLitCap) {
    cLitCap = cLitCap ? cLitCap * 2 : 64;
    cLitCache = realloc(cLitCache, sizeof(TsType *) * (size_t)cLitCap);
    if (!cLitCache) abort();
  }
  cLitCache[cLitN++] = l;
  return l;
}

/* the declared type of an interface or class symbol (getDeclaredTypeOfClassOrInterface) */
static TsType *cDeclaredTypeOfClassOrInterface(TsSymbol *sym) {
  sym = cGetMergedSymbol(sym);
  if (sym->declaredType) return sym->declaredType;
  /* the type parameters come from the first declaration (getOuterTypeParameters + local ones) */
  TsNode *first = sym->decls[0];
  TsType *t = cNewType(TF_Object);
  t->objectFlags = (sym->flags & SF_Class) ? OF_Class : OF_Interface;
  t->symbol = sym;
  sym->declaredType = t;
  int ntp = 0;
  for (int d = 0; d < sym->ndecls; d++) {
    TsNode *dn = sym->decls[d];
    if (dn->typeParameters) ntp += dn->typeParameters->n;
  }
  if (ntp) {
    t->typeParams = ts_arena_alloc(sizeof(TsType *) * (size_t)ntp);
    for (int d = 0; d < sym->ndecls; d++) {
      TsNode *dn = sym->decls[d];
      for (int i = 0; dn->typeParameters && i < dn->typeParameters->n; i++) {
        TsNode *tp = dn->typeParameters->v[i];
        bool seen = false;
        for (int k = 0; k < t->ntypeParams && !seen; k++) seen = t->typeParams[k]->symbol == cGetMergedSymbol(tp->symbol);
        if (!seen) t->typeParams[t->ntypeParams++] = cTypeOfTypeParameter(cGetMergedSymbol(tp->symbol));
      }
    }
  }
  (void)first;
  return t;
}

/* type aliases being resolved: a cycle through eagerly resolved positions is not in the ported subset
 * (TypeScript defers those references, or reports 2456) */
static TsSymbol *cAliasStack[64];
static int cAliasDepth;

static TsType *cDeclaredTypeOfTypeAlias(TsSymbol *sym) {
  if (sym->declaredType) return sym->declaredType;
  TsNode *d = sym->decls[0];
  if (d->typeParameters && d->typeParameters->n) return cUnported("generic type alias");
  for (int i = 0; i < cAliasDepth; i++) if (cAliasStack[i] == sym) return cUnported("circular type alias");
  if (cAliasDepth == 64) return cUnported("alias depth");
  cAliasStack[cAliasDepth++] = sym;
  TsType *t = cTypeFromNode(d->type);
  cAliasDepth--;
  if (t == cIntrinsicMarkerType) {
    /* getDeclaredTypeOfTypeAlias (61914): BuiltinIteratorReturn is undefined under strictBuiltinIteratorReturn,
     * which the project's tsconfig gets from "strict": true */
    if (bnEq(sym->name, bnC("BuiltinIteratorReturn"))) t = cUndefinedType;
    else t = cUnported("intrinsic type alias");
  }
  sym->declaredType = t;
  return t;
}

/* the default type node of a type parameter (its declaration with a default; the merged declarations) */
static TsNode *cTypeParamDefaultNode(TsType *tp) {
  TsSymbol *sym = tp->symbol;
  for (int i = 0; i < sym->ndecls; i++)
    if (sym->decls[i]->defaultType) return sym->decls[i]->defaultType;
  return NULL;
}

/* the type of an enum member used as a type (a literal type of the enum; its value is not computed) */
static TsType *cEnumMemberType(TsSymbol *m) {
  m = cGetMergedSymbol(m);
  if (m->declaredType) return m->declaredType;
  TsNode *d = m->decls[0];
  uint32_t fl = (d->initializer && d->initializer->kind == TSK_StringLiteral) ? TF_StringLiteral : TF_NumberLiteral;
  TsType *t = cNewType(fl | TF_EnumLiteral);
  t->symbol = m;
  m->declaredType = t;
  return t;
}

/* the type arguments of a reference to a generic declaration: the given ones, then the defaults (fillMissing) */
static TsType **cTypeArgsOfReference(TsType **params, int ntp, TsNode *ref, int *nout) {
  int na = ref->typeArguments ? ref->typeArguments->n : 0;
  if (na > ntp) return NULL;
  TsType **args = ts_arena_alloc(sizeof(TsType *) * (size_t)(ntp ? ntp : 1));
  for (int i = 0; i < na; i++) {
    args[i] = cTypeFromNode(ref->typeArguments->v[i]);
    if (args[i] == cErrorType) return NULL;
  }
  for (int i = na; i < ntp; i++) {
    TsNode *dn = cTypeParamDefaultNode(params[i]);
    if (!dn) return NULL;
    TsType *def = cTypeFromNode(dn);
    if (def == cErrorType) return NULL;
    if (i > 0) def = cInstantiate(def, cCreateTypeMapper(params, args, i));
    args[i] = def;
  }
  *nout = ntp;
  return args;
}

/* getDeclaredTypeOfTypeAlias for a generic alias, instantiated per type arguments (getTypeAliasInstantiation):
 * the body is the declared body under a mapper from the parameters to the arguments. The instantiations are
 * kept on a holder type of the alias. */
static TsType *cGenericAliasReference(TsSymbol *sym, TsNode *ref) {
  TsNode *ad = sym->decls[0];
  int ntp = ad->typeParameters->n;
  if (!sym->aliasHolder) {
    TsType *h = cNewType(TF_Any);
    h->typeParams = ts_arena_alloc(sizeof(TsType *) * (size_t)ntp);
    h->ntypeParams = 0;
    for (int i = 0; i < ntp; i++) {
      TsNode *tpn = ad->typeParameters->v[i];
      h->typeParams[h->ntypeParams++] = cTypeOfTypeParameter(cGetMergedSymbol(tpn->symbol));
    }
    sym->aliasHolder = h;
  }
  TsType *h = sym->aliasHolder;
  int n;
  TsType **args = cTypeArgsOfReference(h->typeParams, ntp, ref, &n);
  if (!args) return cUnported("generic alias arguments");
  TsInst *e = cInstFind(h, args, n);
  if (e) return e->val;
  TsType *body = cTypeFromNode(ad->type);
  TsType *r = body == cErrorType ? body : cInstantiate(body, cCreateTypeMapper(h->typeParams, args, n));
  cInstAdd(h, args, n, r);
  return r;
}

/* a type reference to a symbol with type arguments */
static TsType *cTypeFromSymbolReference(TsSymbol *s, TsNode *ref) {
  s = cGetMergedSymbol(s);
  if (s->flags & SF_TypeAlias) {
    TsNode *ad = cGetMergedSymbol(s)->decls[0];
    int ntp = ad->typeParameters ? ad->typeParameters->n : 0;
    if (ntp == 0) {
      if (ref->typeArguments && ref->typeArguments->n) return cUnported("type arguments of a non-generic alias");
      return cDeclaredTypeOfTypeAlias(s);
    }
    return cGenericAliasReference(cGetMergedSymbol(s), ref);
  }
  if (s->flags & SF_EnumMember) return cEnumMemberType(s);
  if (s->flags & (SF_Class | SF_Interface)) {
    TsType *d = cDeclaredTypeOfClassOrInterface(s);
    int na = ref->typeArguments ? ref->typeArguments->n : 0;
    if (d->ntypeParams == 0 && na == 0) return d;
    if (na > d->ntypeParams) return cUnported("type argument count");
    TsType **args = ts_arena_alloc(sizeof(TsType *) * (size_t)d->ntypeParams);
    for (int i = 0; i < na; i++) {
      args[i] = cTypeFromNode(ref->typeArguments->v[i]);
      if (args[i] == cErrorType) return cErrorType;
    }
    /* fillMissingTypeArguments (64560): the defaults, instantiated with the arguments before them */
    for (int i = na; i < d->ntypeParams; i++) {
      TsNode *dn = cTypeParamDefaultNode(d->typeParams[i]);
      if (!dn) return cUnported("missing type argument without default");
      TsType *def = cTypeFromNode(dn);
      if (def == cErrorType) return cErrorType;
      if (i > 0) def = cInstantiate(def, cCreateTypeMapper(d->typeParams, args, i));
      args[i] = def;
    }
    return cTypeReference(d, args, d->ntypeParams);
  }
  if (s->flags & SF_TypeParameter) return cTypeOfTypeParameter(s);
  return cUnported("type reference kind");
}

/* the symbol of an entity name (resolveEntityName): an identifier with the meaning, or a qualified name whose
 * left side is a namespace and whose right side is one of its exports. Import aliases are followed. */
static TsSymbol *cEntityNameSymbol(TsNode *loc, TsNode *name, uint32_t meaning) {
  TsSymbol *s;
  if (name->kind == TSK_Identifier) {
    s = cResolveNameMeaning(loc, bnEscapeText(name->text, name->textLen), meaning);
  } else if (name->kind == TSK_QualifiedName || name->kind == TSK_PropertyAccessExpression) {
    TsNode *left = name->kind == TSK_QualifiedName ? name->left : name->expression;
    TsNode *right = name->kind == TSK_QualifiedName ? name->right : name->name;
    TsSymbol *ls = cEntityNameSymbol(loc, left, SF_Namespace | SF_Alias);
    if (!ls) return NULL;
    ls = cGetMergedSymbol(ls);
    if (!ls->exports || !right) return NULL;
    s = bTableGet(ls->exports, bnEscapeText(right->text, right->textLen));
  } else return NULL;
  if (s && (s->flags & SF_Alias)) s = cResolveImportAlias(s, 0);
  return s;
}

/* the symbol a TypeReference names (type meaning), following import aliases */
static TsSymbol *cSymbolOfTypeName(TsNode *loc, TsNode *name) {
  if (name->kind != TSK_Identifier && name->kind != TSK_QualifiedName) return NULL;
  return cEntityNameSymbol(loc, name, SF_TypeMeaning);
}

/* globalArrayType: the generic Array interface of the global scripts */
static TsType *cGlobalArrayType;
static TsType *cGlobalArrayDeclared(void) {
  if (!cGlobalArrayType) {
    TsSymbol *a = bTableGet(cGlobals, bnC("Array"));
    if (!a) abort();
    cGlobalArrayType = cDeclaredTypeOfClassOrInterface(a);
  }
  return cGlobalArrayType;
}

/* getSymbolId: symbols get their ids on first use (nextSymbolId) */
static uint32_t cNextSymbolId = 1;
static uint32_t cSymbolId(TsSymbol *s) {
  if (!s->checkId) s->checkId = cNextSymbolId++;
  return s->checkId;
}

/* getESSymbolLikeTypeForNode / createUniqueESSymbolType: `unique symbol` on a readonly property
 * signature (the form used by lib.es2015.symbol.d.ts). The escaped name is "__@<name>@<id>" and is
 * kept in text, which is what late binding uses as the member name (getPropertyNameFromType). */
/* isVarConst with an identifier name in a variable statement (isValidESSymbolDeclaration, 66800) */
static bool cIsConstVariableDecl(TsNode *d) {
  return d->kind == TSK_VariableDeclaration && d->name && d->name->kind == TSK_Identifier && d->parent &&
         d->parent->kind == TSK_VariableDeclarationList && (d->parent->flags & 7u) == TNF_Const;
}

/* isSymbolOrSymbolForCall (66780): Symbol(...) or Symbol.for(...) of the global Symbol */
static bool cIsSymbolCall(TsNode *e) {
  if (e->kind != TSK_CallExpression || !e->expression) return false;
  TsNode *left = e->expression;
  if (left->kind == TSK_PropertyAccessExpression && left->name && left->name->textLen == 3 &&
      memcmp(left->name->text, "for", 3) == 0)
    left = left->expression;
  if (!left || left->kind != TSK_Identifier || left->textLen != 6 || memcmp(left->text, "Symbol", 6) != 0) return false;
  return cResolveValueName(left, bnC("Symbol")) == bTableGet(cGlobals, bnC("Symbol"));
}

static TsType *cUniqueSymbolType(TsNode *decl) {
  bool valid = (decl->kind == TSK_PropertySignature && (bModifierFlags(decl) & TMF_Readonly)) || cIsConstVariableDecl(decl);
  if (!valid || !decl->symbol) return cUnported("unique symbol placement");
  TsSymbol *sym = cGetMergedSymbol(decl->symbol);
  if (sym->uniqueType) return sym->uniqueType;
  TsType *t = cNewType(TF_UniqueESSymbol);
  t->symbol = sym;
  char *buf = ts_arena_alloc((size_t)sym->name.n + 32);
  int k = snprintf(buf, (size_t)sym->name.n + 32, "__@%.*s@%u", sym->name.n, sym->name.s, (unsigned)cSymbolId(sym));
  t->text = buf;
  t->textLen = k;
  sym->uniqueType = t;
  return t;
}

static TsType *cTypeFromNodeWorker(TsNode *n);
static TsType *cTupleReference(TsType **elems, int n);

/* getTypeFromTypeNode (67684): the type of a type node, cached on the node (links.resolvedType) */
static TsType *cTypeFromNode(TsNode *n) {
  if (!n) return cErrorType;
  if (n->resolvedType) return n->resolvedType;
  TsType *t = cTypeFromNodeWorker(n);
  n->resolvedType = t;
  return t;
}

static TsType *cTypeFromNodeWorker(TsNode *n) {
  if (!n) return cErrorType;
  switch (n->kind) {
    case TSK_AnyKeyword: return cAnyType;
    case TSK_UnknownKeyword: return cUnknownType;
    case TSK_NumberKeyword: return cNumberType;
    case TSK_BigIntKeyword: return cBigIntType;
    case TSK_StringKeyword: return cStringType;
    case TSK_BooleanKeyword: return cBooleanType;
    case TSK_SymbolKeyword: return cESSymbolType;
    case TSK_ObjectKeyword: return cNonPrimitiveType;
    case TSK_UndefinedKeyword: return cUndefinedType;
    case TSK_NeverKeyword: return cNeverType;
    case TSK_VoidKeyword: return cVoidType;
    case TSK_IntrinsicKeyword: return cIntrinsicMarkerType;
    case TSK_ParenthesizedType: return cTypeFromNode(n->type);
    case TSK_TypeOperator:
      if (n->extra != TSK_UniqueKeyword) return cUnported("type operator");
      if (!n->type || n->type->kind != TSK_SymbolKeyword) return cUnported("unique operand");
      return cUniqueSymbolType(n->parent);
    case TSK_LiteralType: {
      TsNode *l = n->literal;
      if (!l) return cUnported("literal type");
      if (l->kind == TSK_NullKeyword) return cNullType;
      if (l->kind == TSK_TrueKeyword) return cTrueType;
      if (l->kind == TSK_FalseKeyword) return cFalseType;
      if (l->kind == TSK_StringLiteral) return cLiteralType(TF_StringLiteral, l->text, l->textLen);
      if (l->kind == TSK_NumericLiteral) return cLiteralType(TF_NumberLiteral, l->text, l->textLen);
      return cUnported("literal type value");
    }
    case TSK_UnionType: case TSK_IntersectionType: {
      uint32_t fl = n->kind == TSK_UnionType ? TF_Union : TF_Intersection;
      int cnt = n->types ? n->types->n : 0;
      TsType **members = ts_arena_alloc(sizeof(TsType *) * (size_t)(cnt ? cnt : 1));
      for (int i = 0; i < cnt; i++) {
        TsType *m = cTypeFromNode(n->types->v[i]);
        if (m == cErrorType) return cErrorType;
        members[i] = m;
      }
      return fl == TF_Union ? cGetUnionType(members, cnt) : cGetIntersectionType(members, cnt);
    }
    case TSK_TupleType: {
      /* the element types of a tuple of plain types (named, optional and rest members are not ported) */
      int cnt = n->elements ? n->elements->n : 0;
      TsType **el = ts_arena_alloc(sizeof(TsType *) * (size_t)(cnt ? cnt : 1));
      for (int i = 0; i < cnt; i++) {
        TsNode *e = n->elements->v[i];
        if (e->kind == TSK_NamedTupleMember || e->kind == TSK_OptionalType || e->kind == TSK_RestType)
          return cUnported("tuple member kind");
        el[i] = cTypeFromNode(e);
        if (el[i] == cErrorType) return cErrorType;
      }
      return cTupleReference(el, cnt);
    }
    case TSK_ArrayType: {
      /* createArrayType: Array<T>, a reference to the global generic interface */
      TsType *el = cTypeFromNode(n->elementType);
      if (el == cErrorType) return cErrorType;
      return cTypeReference(cGlobalArrayDeclared(), &el, 1);
    }
    case TSK_TypeLiteral: {
      TsSymbol *s = cGetMergedSymbol(n->symbol);
      if (!s) return cUnported("type literal without symbol");
      if (s->declaredType) return s->declaredType;
      TsType *t = cNewType(TF_Object);
      t->objectFlags = OF_Anonymous;
      t->symbol = s;
      t->typeNode = n;
      s->declaredType = t;
      return t;
    }
    case TSK_FunctionType: case TSK_ConstructorType: {
      TsSymbol *s = cGetMergedSymbol(n->symbol);
      if (!s) return cUnported("function type without symbol");
      if (s->declaredType) return s->declaredType;
      TsType *t = cNewType(TF_Object);
      t->objectFlags = OF_Anonymous;
      t->symbol = s;
      t->typeNode = n;
      s->declaredType = t;
      return t;
    }
    case TSK_TypeReference: {
      TsSymbol *s = cSymbolOfTypeName(n, n->typeName);
      if (!s && n->typeName && n->typeName->kind == TSK_QualifiedName) {
        /* an enum member (SyntaxKind.EqualsToken) is a type meaning of the enum's exports */
        TsSymbol *m = cEntityNameSymbol(n, n->typeName, SF_EnumMember);
        if (m && (m->flags & SF_EnumMember)) return cEnumMemberType(m);
      }
      if (!s) return cUnported("unresolved type name");
      return cTypeFromSymbolReference(s, n);
    }
    case TSK_TypeQuery: {
      TsNode *e = n->exprName;
      if (e && e->kind == TSK_Identifier) {
        TsSymbol *s = cResolveValueName(n, bnEscapeText(e->text, e->textLen));
        if (s && s == cGlobalThisSymbol) return cGlobalThisType;
        if (s && !(s->flags & SF_Alias)) return cTypeOfSymbol(s);
      }
      return cUnported("type query");
    }
    default: break;
  }
  return cUnported("type node kind");
}

/* tuple targets by arity (createTupleTargetType): the target has one type parameter per element and its base
 * type is Array<T0 | T1 | ...>, so the members of a tuple are those of the array; element properties are not
 * ported */
static TsType *cTupleTargets[16];
static TsType *cTupleTarget(int arity) {
  if (arity < 16 && cTupleTargets[arity]) return cTupleTargets[arity];
  TsType *tg = cNewType(TF_Object);
  tg->objectFlags = OF_Tuple | OF_Interface;
  TsSymbol *sym = cCreateSymbol(SF_TypeLiteral, bnC("__tuple"));
  tg->symbol = sym;
  tg->typeParams = ts_arena_alloc(sizeof(TsType *) * (size_t)(arity ? arity : 1));
  tg->ntypeParams = arity;
  for (int i = 0; i < arity; i++) {
    char *nm = ts_arena_alloc(16);
    int k = snprintf(nm, 16, "T%d", i);
    TsSymbol *ps = cCreateSymbol(SF_TypeParameter, (BName){ nm, k });
    TsType *p = cNewType(TF_TypeParameter);
    p->symbol = ps;
    tg->typeParams[i] = p;
  }
  TsType *elem = cNeverType;
  if (arity) elem = cGetUnionType(tg->typeParams, arity);
  tg->bases = ts_arena_alloc(sizeof(TsType *));
  tg->bases[0] = cTypeReference(cGlobalArrayDeclared(), &elem, 1);
  tg->nbases = 1;
  tg->basesDone = true;
  if (arity < 16) cTupleTargets[arity] = tg;
  return tg;
}

static TsType *cTupleReference(TsType **elems, int n) {
  TsType *tg = cTupleTarget(n);
  TsType *r = cTypeReference(tg, elems, n);
  r->objectFlags |= OF_Tuple;
  return r;
}

/* ---- members ---- */
static bool cIsSignatureName(BName n) {
  return (n.n == 6 && memcmp(n.s, "__call", 6) == 0) || (n.n == 5 && memcmp(n.s, "__new", 5) == 0) ||
         (n.n == 7 && memcmp(n.s, "__index", 7) == 0);
}

/* isReadonlySymbol: a readonly modifier on a property declaration */
static bool cIsReadonlyProp(TsSymbol *s) {
  for (int i = 0; i < s->ndecls; i++) if (bModifierFlags(s->decls[i]) & TMF_Readonly) return true;
  return false;
}

/* ---- late-bound members (getMembersOfSymbol, resolveDeclaredMembers) ---- */
static TsSymbol *cPropertyOf(TsType *t, BName name);

/* isLateBindableName: a computed name whose expression is an entity name (Symbol.iterator, x) */
static bool cIsLateBindableName(TsNode *name) {
  return name->kind == TSK_ComputedPropertyName && name->expression && bIsEntityNameExpression(name->expression);
}

/* checkComputedPropertyName of an entity name expression (Symbol.iterator, x.y, x): the type of the
 * name. An unresolved or unported part gives the error type. */
static TsType *cEntityNameType(TsNode *loc, TsNode *e) {
  if (e->kind == TSK_Identifier) {
    TsSymbol *s = cResolveValueName(loc, bnEscapeText(e->text, e->textLen));
    if (!s) return cUnported("late name: unresolved identifier");
    return cTypeOfSymbol(s);
  }
  TsType *lt = cEntityNameType(loc, e->expression);
  if (lt == cErrorType) return lt;
  if (!(lt->flags & TF_Object)) return cUnported("late name: property of a non-object");
  TsSymbol *p = cPropertyOf(lt, bnEscapeText(e->name->text, e->name->textLen));
  if (!p) return cUnported("late name: property");
  return cTypeOfSymbol(p);
}

/* the late member name of a computed member: the unique symbol type, NULL when the name type is not
 * usable as a property name (the member is then left out, as lateBindMember does), or the error type */
static TsType *cLateNameType(TsNode *member) {
  TsType *t = cEntityNameType(member->name, member->name->expression);
  if (t == cErrorType) return t;
  return (t->flags & TF_UniqueESSymbol) ? t : NULL;
}

/* getMembersOfSymbol: the early (declared) members, then the late-bound members of the declarations,
 * as getResolvedMembersOrExportsOfSymbol combines them. Returns early itself when there are none. */
static TsSymTable *cMembersWithLate(TsSymbol *s, TsSymTable *early) {
  TsSymTable *lateSyms = NULL; /* the late symbols by name, in the order of lateBindMember calls */
  for (int d = 0; d < s->ndecls; d++) {
    TsNode *dn = s->decls[d];
    if (!dn->members) continue;
    for (int m = 0; m < dn->members->n; m++) {
      TsNode *mem = dn->members->v[m];
      if (!mem->name || !cIsLateBindableName(mem->name)) continue;
      if (bHasSyntacticModifier(mem, TMF_Static) || !mem->symbol) continue; /* static members: resolvedExports */
      TsType *nt = cLateNameType(mem);
      if (nt == cErrorType) { cUnported("late-bound member name"); continue; }
      if (!nt) continue;
      BName key = { nt->text, nt->textLen };
      if (early && bTableGet(early, key)) { cUnported("late-bound member with an early name"); continue; }
      if (!lateSyms) lateSyms = bTableNew();
      TsSymbol *ls = bTableGet(lateSyms, key);
      uint32_t symFlags = mem->symbol->flags;
      if (!ls) {
        ls = cCreateSymbol(0, key);
        bTableSet(lateSyms, key, ls);
      } else if (ls->flags & cExcludedFlags(symFlags)) {
        cUnported("late-bound duplicate member");
        continue;
      }
      ls->flags |= symFlags;
      cAddDeclRaw(ls, mem);
      if (symFlags & SF_Value) bSetValueDeclaration(ls, mem);
    }
  }
  if (!lateSyms) return early;
  TsSymTable *res = early ? cTableCopy(early) : bTableNew();
  for (int i = 0; i < lateSyms->n; i++) bTableSet(res, lateSyms->e[i].key, lateSyms->e[i].sym);
  return res;
}

/* the declared members of an interface, class or type literal: its own symbols and the late-bound ones */
static TsSymTable *cDeclaredMembers(TsType *d) {
  if (!d->declDone) {
    d->declDone = true;
    d->declMembers = cMembersWithLate(d->symbol, d->symbol->members);
  }
  return d->declMembers;
}

/* the base type of `class C extends V` without type arguments when V is a value (getBaseConstructorType,
 * 64.. : the construct signatures of V whose type parameters all have defaults give the instance type) */
static void cResolveSignatures(TsType *t);

static TsType *cBaseFromConstructSignatures(TsSymbol *vs) {
  TsType *vt = cTypeOfSymbol(vs);
  if (vt == cErrorType || !(vt->flags & TF_Object)) return cUnported("base constructor type");
  cResolveSignatures(vt);
  for (int i = 0; i < vt->nconstruct; i++) {
    TsNode *d = vt->construct[i].decl;
    bool generic = false;
    for (int k = 0; d->typeParameters && k < d->typeParameters->n; k++)
      if (!d->typeParameters->v[k]->defaultType) generic = true;
    if (generic) continue;
    if (!d->type) return cUnported("construct signature without return type");
    return cTypeFromNodeM(d->type, vt->construct[i].mapper);
  }
  return cUnported("base constructor without type arguments");
}

/* the symbol of an extends clause expression: an identifier (type meaning), or globalThis.Name */
static TsSymbol *cBaseSymbol(TsNode *loc, TsNode *expr) {
  if (expr->kind == TSK_Identifier) return cSymbolOfTypeName(loc, expr);
  if (expr->kind == TSK_PropertyAccessExpression && expr->expression && expr->expression->kind == TSK_Identifier &&
      expr->expression->textLen != 10 && cEntityNameSymbol(loc, expr->expression, SF_Namespace | SF_Alias)) {
    /* a namespace-qualified base (JsTyping.TypingResolutionHost) */
    return cEntityNameSymbol(loc, expr, SF_TypeMeaning);
  }
  if (expr->kind == TSK_PropertyAccessExpression && expr->expression && expr->expression->kind == TSK_Identifier &&
      expr->expression->textLen == 10 && memcmp(expr->expression->text, "globalThis", 10) == 0 && expr->name &&
      cResolveValueName(loc, bnC("globalThis")) == cGlobalThisSymbol) {
    TsSymbol *g = bTableGet(cGlobals, bnEscapeText(expr->name->text, expr->name->textLen));
    return g && (g->flags & (SF_TypeMeaning | SF_Alias)) ? g : NULL;
  }
  return NULL;
}

/* getBaseTypes (61714) for interfaces and classes: the types named by the extends clauses */
static void cResolveBaseTypes(TsType *d) {
  if (d->basesDone) return;
  d->basesDone = true;
  TsSymbol *s = d->symbol;
  TsType *found[64];
  int nf = 0;
  for (int dd = 0; dd < s->ndecls; dd++) {
    TsNode *dn = s->decls[dd];
    for (int h = 0; dn->heritageClauses && h < dn->heritageClauses->n; h++) {
      TsNode *hc = dn->heritageClauses->v[h];
      if (hc->extra != TSK_ExtendsKeyword) continue;
      for (int e = 0; hc->types && e < hc->types->n; e++) {
        TsNode *ewta = hc->types->v[e];
        TsNode *expr = ewta->expression;
        if (!expr) { cUnported("base type expression"); continue; }
        TsType *bt;
        TsSymbol *vs = (dn->kind == TSK_ClassDeclaration || dn->kind == TSK_ClassExpression) && expr->kind == TSK_Identifier &&
                       !ewta->typeArguments ? cResolveValueName(dn, bnEscapeText(expr->text, expr->textLen)) : NULL;
        if (vs && !(vs->flags & SF_Class)) {
          bt = cBaseFromConstructSignatures(vs);   /* a value such as Map (MapConstructor) */
        } else {
          TsSymbol *bs = cBaseSymbol(dn, expr);
          if (!bs) { cUnported("unresolved base"); continue; }
          bt = cTypeFromSymbolReference(bs, ewta);
        }
        if (bt == cErrorType) continue;
        if (nf == 64) { cUnported("too many base types"); continue; }
        found[nf++] = bt;
      }
    }
  }
  d->bases = cCopyTypes(found, nf);
  d->nbases = nf;
}

static void cResolveMembers(TsType *t);

/* resolveObjectTypeMembers (62289): the members of t are the declared members of src, instantiated
 * under m (the identity when m is NULL), then the inherited members of the instantiated base types. */
static void cResolveObjectMembers(TsType *t, TsType *src, TsMapper *m) {
  TsSymTable *own = m ? cInstantiateTable(cDeclaredMembers(src), m) : cDeclaredMembers(src);
  if (!own) own = bTableNew();
  cResolveBaseTypes(src);
  TsSymTable *res = src->nbases ? cTableCopy(own) : own;
  for (int i = 0; i < src->nbases; i++) {
    TsType *bi = m ? cInstantiate(src->bases[i], m) : src->bases[i];
    if (bi == cErrorType) continue;
    cResolveMembers(bi);
    if (!bi->members) continue;
    for (int k = 0; k < bi->members->n; k++) {
      BName key = bi->members->e[k].key;
      if (cIsSignatureName(key) || !(bi->members->e[k].sym->flags & SF_Value)) continue;
      if (bTableIndex(res, key) < 0) bTableSet(res, key, bi->members->e[k].sym);
    }
  }
  t->members = res;
  t->sigMapper = m;
}

/* resolved members of an object type (resolveStructuredTypeMembers) */
static void cResolveMembers(TsType *t) {
  if (t->resolved) return;
  t->resolved = true;
  if (t->objectFlags & OF_Reference) {
    /* resolveTypeReferenceMembers (62338): the identity when the arguments are the type's parameters */
    TsType *tg = t->target;
    bool identity = t->nargs == tg->ntypeParams;
    for (int i = 0; identity && i < t->nargs; i++) identity = t->args[i] == tg->typeParams[i];
    cResolveObjectMembers(t, tg, identity ? NULL : cCreateTypeMapper(tg->typeParams, t->args, t->nargs));
    return;
  }
  if (t->target) {
    /* resolveAnonymousTypeMembers (62809): the members of the target under the instantiation's mapper */
    cResolveMembers(t->target);
    t->members = cInstantiateTable(t->target->members, t->mapper);
    t->sigMapper = t->mapper;
    return;
  }
  if (t->typeNode && (t->typeNode->kind == TSK_TypeLiteral)) {
    t->members = cDeclaredMembers(t);
    return;
  }
  if (t->objectFlags & (OF_Interface | OF_Class)) {
    cResolveObjectMembers(t, t, NULL);
    return;
  }
  if (t->symbol && (t->symbol->flags & SF_Class)) { t->members = t->symbol->exports; return; }
  t->members = t->symbol ? t->symbol->members : NULL;
}

/* getPropertiesOfObjectType: the property symbols in member order (signatures excluded) */
static int cPropertiesOf(TsType *t, TsSymbol ***out) {
  cResolveMembers(t);
  int n = 0;
  if (!t->members) { *out = NULL; return 0; }
  TsSymbol **v = ts_arena_alloc(sizeof(TsSymbol *) * (size_t)(t->members->n ? t->members->n : 1));
  for (int i = 0; i < t->members->n; i++) {
    TsSymbol *s = t->members->e[i].sym;
    if (!(s->flags & SF_Value) || (s->flags & SF_Signature)) continue; /* getNamedMembers: symbolIsValue */
    if (cIsSignatureName(t->members->e[i].key)) continue;
    v[n++] = s;
  }
  *out = v;
  return n;
}

static TsSymbol *cPropertyOf(TsType *t, BName name) {
  cResolveMembers(t);
  if (!t->members) return NULL;
  TsSymbol *s = bTableGet(t->members, name);
  if (s && (!(s->flags & SF_Value) || (s->flags & SF_Signature))) return NULL;
  return s;
}

/* signatures of an object type: from the declarations of __call / __new, or the declarations of a
 * function or method symbol, or the type node of an anonymous function type. Each signature carries
 * the mapper of the type (instantiateSignatures for instantiated types). */
static void cResolveSignatures(TsType *t) {
  if (t->sigsResolved) return;
  t->sigsResolved = true;
  cResolveMembers(t);
  if (t->target) {
    /* an instantiated anonymous type: the target's signatures, instantiated with the type's mapper */
    cResolveSignatures(t->target);
    t->ncall = t->target->ncall;
    t->nconstruct = t->target->nconstruct;
    t->call = ts_arena_alloc(sizeof(CSig) * (size_t)(t->ncall ? t->ncall : 1));
    for (int i = 0; i < t->ncall; i++) { t->call[i] = t->target->call[i]; t->call[i].mapper = t->mapper; }
    t->construct = ts_arena_alloc(sizeof(CSig) * (size_t)(t->nconstruct ? t->nconstruct : 1));
    for (int i = 0; i < t->nconstruct; i++) { t->construct[i] = t->target->construct[i]; t->construct[i].mapper = t->mapper; }
    return;
  }
  TsSymbol *callSym = NULL, *newSym = NULL;
  if (t->typeNode && (t->typeNode->kind == TSK_FunctionType || t->typeNode->kind == TSK_ConstructorType)) {
    bool ctor = t->typeNode->kind == TSK_ConstructorType;
    t->call = ts_arena_alloc(sizeof(CSig));
    t->call[0].decl = t->typeNode;
    t->call[0].isConstruct = ctor;
    t->call[0].mapper = t->sigMapper;
    t->ncall = ctor ? 0 : 1;
    t->construct = ctor ? t->call : NULL;
    t->nconstruct = ctor ? 1 : 0;
    t->call = ctor ? NULL : t->call;
    return;
  }
  if (t->objectFlags & OF_Anonymous && t->symbol && (t->symbol->flags & (SF_Method | SF_Function))) {
    TsSymbol *s = t->symbol;
    t->call = ts_arena_alloc(sizeof(CSig) * (size_t)(s->ndecls ? s->ndecls : 1));
    t->ncall = 0;
    for (int d = 0; d < s->ndecls; d++) {
      t->call[t->ncall].decl = s->decls[d];
      t->call[t->ncall].isConstruct = false;
      t->call[t->ncall].mapper = t->sigMapper;
      t->ncall++;
    }
    return;
  }
  if (t->members) {
    callSym = bTableGet(t->members, bnC("__call"));
    newSym = bTableGet(t->members, bnC("__new"));
  }
  if (callSym) {
    t->call = ts_arena_alloc(sizeof(CSig) * (size_t)callSym->ndecls);
    for (int d = 0; d < callSym->ndecls; d++) {
      t->call[t->ncall].decl = callSym->decls[d];
      t->call[t->ncall].isConstruct = false;
      t->call[t->ncall].mapper = t->sigMapper;
      t->ncall++;
    }
  }
  if (newSym) {
    t->construct = ts_arena_alloc(sizeof(CSig) * (size_t)newSym->ndecls);
    for (int d = 0; d < newSym->ndecls; d++) {
      t->construct[t->nconstruct].decl = newSym->decls[d];
      t->construct[t->nconstruct].isConstruct = true;
      t->construct[t->nconstruct].mapper = t->sigMapper;
      t->nconstruct++;
    }
  }
}

/* index infos: the __index declarations with one parameter, instantiated with the type's mapper */
static void cResolveIndexInfos(TsType *t) {
  if (t->index || t->nindex) return;
  cResolveMembers(t);
  if (!t->members) return;
  TsSymbol *idx = bTableGet(t->members, bnC("__index"));
  if (!idx) return;
  t->index = ts_arena_alloc(sizeof(CIndexInfo) * (size_t)idx->ndecls);
  for (int d = 0; d < idx->ndecls; d++) {
    TsNode *decl = idx->decls[d];
    if (decl->parameters->n != 1) { cUnported("index signature arity"); continue; }
    CIndexInfo ii;
    ii.keyType = cTypeFromNodeM(decl->parameters->v[0]->type, t->sigMapper);
    ii.type = cTypeFromNodeM(decl->type, t->sigMapper);
    ii.isReadonly = (bModifierFlags(decl) & TMF_Readonly) != 0;
    t->index[t->nindex++] = ii;
  }
}

/* ---- types of symbols (getTypeOfSymbol) ---- */
static TsType *cTypeOfSymbol(TsSymbol *sym) {
  if (sym->instTarget) {   /* getTypeOfInstantiatedSymbol (61381) */
    if (!sym->type) sym->type = cInstantiate(cTypeOfSymbol(sym->instTarget), sym->instMapper);
    return sym->type;
  }
  sym = cGetMergedSymbol(sym);
  if (sym->type) return sym->type;
  TsType *t = NULL;
  if (sym->flags & SF_Alias) {
    TsSymbol *target = cResolveImportAlias(sym, 0);
    t = target ? cTypeOfSymbol(target) : cErrorType;
  } else if (sym->flags & SF_Variable) {
    TsNode *d = sym->valueDeclaration ? sym->valueDeclaration : sym->decls[0];
    if (d->kind != TSK_VariableDeclaration) t = cUnported("variable declaration kind");
    else if (d->type) t = cTypeFromNode(d->type);
    else if (cIsConstVariableDecl(d) && d->initializer && cIsSymbolCall(d->initializer)) t = cUniqueSymbolType(d);
    else t = cUnported("variable without annotation");
  } else if (sym->flags & SF_Property) {
    TsNode *d = sym->valueDeclaration ? sym->valueDeclaration : sym->decls[0];
    if ((d->kind != TSK_PropertySignature && d->kind != TSK_PropertyDeclaration) || !d->type) t = cUnported("property without annotation");
    else {
      t = cTypeFromNode(d->type);
      if (d->questionToken && t != cErrorType) {
        /* addOptionality: T | undefined under strictNullChecks */
        TsType *u = cNewType(TF_Union);
        u->types = ts_arena_alloc(sizeof(TsType *) * 2);
        u->types[0] = t;
        u->types[1] = cUndefinedType;
        u->ntypes = 2;
        t = u;
      }
    }
  } else if (sym->flags & SF_Accessor) {
    /* the type of an accessor: the return type of the getter, else the parameter type of the setter */
    TsNode *typed = NULL;
    for (int d = 0; d < sym->ndecls && !typed; d++)
      if (sym->decls[d]->kind == TSK_GetAccessor && sym->decls[d]->type) typed = sym->decls[d];
    for (int d = 0; d < sym->ndecls && !typed; d++)
      if (sym->decls[d]->kind == TSK_SetAccessor && sym->decls[d]->parameters && sym->decls[d]->parameters->n) {
        TsNode *p = sym->decls[d]->parameters->v[0];
        if (p->type) { typed = p; }
      }
    if (!typed) t = cUnported("accessor without a type");
    else if (typed->kind == TSK_Parameter) t = cTypeFromNode(typed->type);
    else t = cTypeFromNode(typed->type);
  } else if (sym->flags & SF_Class) {
    /* the constructor type of a class (getTypeOfFuncClassEnumModule): its static members and prototype */
    TsType *a = cNewType(TF_Object);
    a->objectFlags = OF_Anonymous;
    a->symbol = sym;
    t = a;
  } else if (sym->flags & (SF_Method | SF_Function)) {
    /* an anonymous object type with the signatures of the declarations */
    TsType *a = cNewType(TF_Object);
    a->objectFlags = OF_Anonymous;
    a->symbol = sym;
    t = a;
  } else {
    t = cUnported("symbol kind");
  }
  sym->type = t;
  return t;
}

/* ---- the identity relation ---- */
typedef struct { TsType *s, *t; bool r; } CIdPair;
static CIdPair *cIdCache;
static int cIdN, cIdCap;
static CIdPair cIdentityStack[256];
static int cIdDepth;

static bool cIdentical(TsType *s, TsType *t);

static int cIdFind(TsType *s, TsType *t) {
  for (int i = 0; i < cIdN; i++) if (cIdCache[i].s == s && cIdCache[i].t == t) return i;
  return -1;
}

static bool cSignaturesIdentical(TsType *s, TsType *t, bool isConstruct);

static bool cCompareSignaturesIdentical(CSig *a, CSig *b);

static bool cIdentityStructural(TsType *s, TsType *t) {
  /* propertiesIdenticalTo */
  TsSymbol **sp, **tp;
  int ns = cPropertiesOf(s, &sp), nt = cPropertiesOf(t, &tp);
  if (ns != nt) return false;
  for (int i = 0; i < ns; i++) {
    TsSymbol *tprop = cPropertyOf(t, sp[i]->name);
    if (!tprop) return false;
    TsSymbol *sprop = sp[i];
    if (sprop == tprop) continue;
    if ((sprop->flags & SF_Optional) != (tprop->flags & SF_Optional)) return false;
    if (cIsReadonlyProp(sprop) != cIsReadonlyProp(tprop)) return false;
    if (cTypeOfSymbol(sprop) == cErrorType || cTypeOfSymbol(tprop) == cErrorType) { cUnportedHit = true; return true; }
    if (!cIdentical(cTypeOfSymbol(sprop), cTypeOfSymbol(tprop))) return false;
  }
  /* signaturesIdenticalTo: call, then construct */
  cResolveSignatures(s);
  cResolveSignatures(t);
  if (!cSignaturesIdentical(s, t, false)) return false;
  if (!cSignaturesIdentical(s, t, true)) return false;
  /* indexSignaturesIdenticalTo */
  cResolveIndexInfos(s);
  cResolveIndexInfos(t);
  if (s->nindex != t->nindex) return false;
  for (int i = 0; i < t->nindex; i++) {
    int found = -1;
    for (int j = 0; j < s->nindex && found < 0; j++)
      if (cIdentical(s->index[j].keyType, t->index[i].keyType)) found = j;
    if (found < 0) return false;
    if (!cIdentical(s->index[found].type, t->index[i].type)) return false;
    if (s->index[found].isReadonly != t->index[i].isReadonly) return false;
  }
  return true;
}

static bool cSignaturesIdentical(TsType *s, TsType *t, bool isConstruct) {
  int ns = isConstruct ? s->nconstruct : s->ncall, nt = isConstruct ? t->nconstruct : t->ncall;
  CSig *ss = isConstruct ? s->construct : s->call, *ts = isConstruct ? t->construct : t->call;
  if (ns != nt) return false;
  for (int i = 0; i < ns; i++)
    if (!cCompareSignaturesIdentical(&ss[i], &ts[i])) return false;
  return true;
}

/* a parameter list without the this parameter: count, min count, rest parameter node */
typedef struct { TsNode *params[64]; int n; int min; TsNode *thisParam; TsNode *rest; } CParams;

static bool cIsThisParam(TsNode *p) {
  return p->name && p->name->kind == TSK_Identifier && p->name->textLen == 4 && memcmp(p->name->text, "this", 4) == 0;
}

static bool cSplitParams(TsNode *decl, CParams *cp) {
  cp->n = 0;
  cp->min = 0;
  cp->thisParam = NULL;
  cp->rest = NULL;
  TsList *pl = decl->parameters;
  for (int i = 0; pl && i < pl->n; i++) {
    TsNode *p = pl->v[i];
    if (cIsThisParam(p)) { cp->thisParam = p; continue; }
    if (cp->n == 64) return false;
    cp->params[cp->n++] = p;
    if (p->dotDotDotToken) cp->rest = p;
    else if (!p->questionToken && !p->initializer) cp->min = cp->n;
  }
  return true;
}

/* the type of a parameter at a position (getTypeAtPosition): a rest parameter gives its element type */
static TsType *cParamTypeAt(CParams *cp, int pos, bool *ok, TsMapper *m) {
  TsNode *p = pos < cp->n ? cp->params[pos] : cp->rest;
  *ok = true;
  if (!p) { *ok = false; return cErrorType; }
  if (p->dotDotDotToken) {
    TsNode *tn = p->type;
    if (tn && tn->kind == TSK_ArrayType) return cTypeFromNodeM(tn->elementType, m);
    if (tn && tn->kind == TSK_TypeReference && tn->typeArguments && tn->typeArguments->n == 1)
      return cTypeFromNodeM(tn->typeArguments->v[0], m);
    return cUnported("rest parameter element type");
  }
  TsType *t = cTypeFromNodeM(p->type, m);
  if (p->questionToken && t != cErrorType) {
    TsType *u = cNewType(TF_Union);
    u->types = ts_arena_alloc(sizeof(TsType *) * 2);
    u->types[0] = t;
    u->types[1] = cUndefinedType;
    u->ntypes = 2;
    t = u;
  }
  return t;
}

static bool cCompareSignaturesIdentical(CSig *a, CSig *b) {
  if (a->decl == b->decl && a->mapper == b->mapper) return true;
  TsNode *da = a->decl, *db = b->decl;
  TsMapper *ma = a->mapper, *mb = b->mapper;
  if ((da->typeParameters && da->typeParameters->n) || (db->typeParameters && db->typeParameters->n)) {
    cUnported("generic signature");
    return true;
  }
  CParams pa, pb;
  if (!cSplitParams(da, &pa) || !cSplitParams(db, &pb)) { cUnported("too many parameters"); return true; }
  /* isMatchingSignature: parameter count, min count and rest parameter */
  bool restA = pa.rest != NULL, restB = pb.rest != NULL;
  if (pa.n != pb.n || pa.min != pb.min || restA != restB) return false;
  /* this types: a target without a this type passes (as in TypeScript) */
  if (pa.thisParam) {
    TsType *ta = cTypeFromNodeM(pa.thisParam->type, ma);
    if (pb.thisParam) {
      TsType *tb = cTypeFromNodeM(pb.thisParam->type, mb);
      if (ta == cErrorType || tb == cErrorType) { cUnported("this parameter"); return true; }
      if (!cIdentical(ta, tb)) return false;
    }
  }
  /* parameter types at every position of the target */
  for (int i = 0; i < pb.n; i++) {
    bool okA, okB;
    TsType *tx = cParamTypeAt(&pa, i, &okA, ma), *ty = cParamTypeAt(&pb, i, &okB, mb);
    if (!okA || !okB || tx == cErrorType || ty == cErrorType) { cUnported("parameter type"); return true; }
    if (!cIdentical(ty, tx)) return false;
  }
  /* return types */
  if ((da->type && da->type->kind == TSK_TypePredicate) || (db->type && db->type->kind == TSK_TypePredicate)) {
    cUnported("type predicate");
    return true;
  }
  TsType *ra = da->type ? cTypeFromNodeM(da->type, ma) : cAnyType;
  TsType *rb = db->type ? cTypeFromNodeM(db->type, mb) : cAnyType;
  if (ra == cErrorType || rb == cErrorType) { cUnported("return type"); return true; }
  return cIdentical(ra, rb);
}

static bool cIdentical(TsType *s, TsType *t) {
  if (s == t) return true;
  if (s == cErrorType || t == cErrorType) { cUnportedHit = true; return true; }
  if (s->flags != t->flags) return false;
  if (s->flags & (TF_Any | TF_Unknown | TF_String | TF_Number | TF_Boolean | TF_BigInt | TF_ESSymbol | TF_Void |
                  TF_Undefined | TF_Null | TF_Never | TF_NonPrimitive))
    return true;
  if (s->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BooleanLiteral | TF_BigIntLiteral)) return false; /* distinct literal objects */
  if (s->flags & (TF_Union | TF_Intersection)) {
    /* eachTypeRelatedToSomeType both ways */
    for (int i = 0; i < s->ntypes; i++) {
      bool hit = false;
      for (int j = 0; j < t->ntypes && !hit; j++) hit = cIdentical(s->types[i], t->types[j]);
      if (!hit) return false;
    }
    for (int j = 0; j < t->ntypes; j++) {
      bool hit = false;
      for (int i = 0; i < s->ntypes && !hit; i++) hit = cIdentical(t->types[j], s->types[i]);
      if (!hit) return false;
    }
    return true;
  }
  if (!(s->flags & TF_Object)) return false;
  /* the identity cache and the stack of pairs being compared (a pair on the stack is assumed related) */
  int ci = cIdFind(s, t);
  if (ci >= 0) return cIdCache[ci].r;
  for (int i = 0; i < cIdDepth; i++)
    if (cIdentityStack[i].s == s && cIdentityStack[i].t == t) return true;
  if (cIdDepth >= 200) { cUnportedHit = true; return true; }
  cIdentityStack[cIdDepth].s = s;
  cIdentityStack[cIdDepth].t = t;
  cIdDepth++;
  bool r;
  if ((s->objectFlags & OF_Reference) && (t->objectFlags & OF_Reference) && s->target == t->target) {
    r = true;
    for (int i = 0; i < s->nargs && r; i++) r = cIdentical(s->args[i], t->args[i]);
  } else if ((s->objectFlags | t->objectFlags) & OF_Reference) {
    cUnported("identity of references with different targets");
    r = true;
  } else {
    r = cIdentityStructural(s, t);
  }
  cIdDepth--;
  if (cIdN == cIdCap) {
    cIdCap = cIdCap ? cIdCap * 2 : 256;
    cIdCache = realloc(cIdCache, sizeof(CIdPair) * (size_t)cIdCap);
    if (!cIdCache) abort();
  }
  cIdCache[cIdN].s = s;
  cIdCache[cIdN].t = t;
  cIdCache[cIdN].r = r;
  cIdN++;
  cIdentityCacheCount++;
  return r;
}

/* Every checker run starts with empty caches: the arena that holds their entries is reset between runs. */
static void cResetTypeCaches(void) {
  cIdN = 0;
  cLitN = 0;
  cGlobalArrayType = NULL;
  cNextSymbolId = 1;
  cUnportedCount = 0;
  cIdentityCacheCount = 0;
  cInstTotal = 0;
  cInstDepth = 0;
  cActiveN = 0;
  memset(cUnionTab, 0, sizeof cUnionTab);
  memset(cTupleTargets, 0, sizeof cTupleTargets);
  memset(cInterTab, 0, sizeof cInterTab);
}

/* isTypeIdenticalTo: the entry point used by the checks */
static bool cIsTypeIdenticalTo(TsType *s, TsType *t) { return cIdentical(s, t); }

#endif
