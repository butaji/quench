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

#include <math.h>
#include "ts-checker-decls.h"

/* TypeFlags (TypeScript 5.9.2) */
enum {
  TF_Any = 1, TF_Unknown = 2, TF_String = 4, TF_Number = 8, TF_Boolean = 16, TF_Enum = 32, TF_BigInt = 64,
  TF_StringLiteral = 128, TF_NumberLiteral = 256, TF_BooleanLiteral = 512, TF_EnumLiteral = 1024,
  TF_BigIntLiteral = 2048, TF_ESSymbol = 4096, TF_UniqueESSymbol = 8192, TF_Void = 16384, TF_Undefined = 32768,
  TF_Null = 65536, TF_Never = 131072, TF_TypeParameter = 262144, TF_Object = 524288, TF_Union = 1048576,
  TF_Intersection = 2097152, TF_Index = 4194304, TF_IndexedAccess = 8388608, TF_NonPrimitive = 67108864,
  TF_Conditional = 16777216, TF_TemplateLiteral = 134217728, TF_StringMapping = 268435456
};

/* ObjectFlags (TypeScript 5.9.2) */
enum { OF_Class = 1, OF_Interface = 2, OF_Reference = 4, OF_Tuple = 8, OF_Anonymous = 16, OF_Mapped = 32, OF_Instantiated = 64,
       OF_ObjectLiteral = 128,
       OF_PrimitiveUnion = 256,   /* unions of no object, any, void, never or intersection members */
       OF_ContainsIntersections = 0x4000000,   /* unions with an intersection member (ObjectFlags.ContainsIntersections) */
       OF_NeverIntComputed = 0x1000000,        /* intersections: IsNeverIntersectionComputed */
       OF_NeverIntersection = 0x2000000,       /* intersections: IsNeverIntersection (a never-reduced property) */
       OF_ReadonlyTuple = 0x8000000 };         /* tuple targets: readonly (createTupleTargetType with readonly) */

typedef struct CSig CSig;
typedef struct CIndexInfo { struct TsType *keyType, *type; bool isReadonly; TsNode *decl; } CIndexInfo;
/* an instantiation of a generic type by type arguments (the type's instantiations list) */
typedef struct TsInst { struct TsType **key; int n; struct TsType *val; } TsInst;

typedef struct { const char *s; int n; } CStr;

typedef struct TsType {
  uint32_t flags, objectFlags;
  int32_t id;
  TsSymbol *symbol;              /* the symbol of an object type, the alias of a union, the param of a type parameter */
  TsSymbol *aliasSymbol;         /* the type alias of a union that is the body of a non-generic alias (getUnionType aliasSymbol) */
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
  bool indexResolved;            /* cResolveIndexInfos has run (own and inherited index infos) */
  struct TsType **instBases;     /* the base types of the members table, instantiated (cResolveObjectMembers) */
  int ninstBases;
  TsInst *insts;                 /* instantiations of this generic type, by type arguments */
  int ninsts, capInsts;
  uint8_t couldContain;          /* couldContainTypeVariables: 0 not computed, 1 yes, 2 no */
  struct TsMapper *sigMapper;    /* the mapper of the signatures and index infos of this type */
  TsSymTable *declMembers;       /* declared members of an interface, class or type literal (own and late) */
  TsSymTable *unionProps;        /* union and intersection types: the propertyCache (getUnionOrIntersectionProperty) */
  struct TsType *resolvedReducedType;   /* unions with intersection members: getReducedType (ts-relate.h) */
  TsSymbol **resolvedProps;      /* union and intersection types: resolvedProperties (getPropertiesOfUnionOrIntersectionType) */
  int nResolvedProps;
  bool resolvedPropsDone;
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
  /* classes and interfaces (Reference targets): the this type parameter (getDeclaredTypeOfClassOrInterface) */
  struct TsType *thisType;
  bool isThisType;
  /* indexed access types (T[K]): object and index type; index types (keyof T): the type */
  struct TsType *iaObj, *iaIdx, *ixOf;
  struct TsType *resolvedIndex;   /* getIndexTypeForGenericType cache */
  /* literal types (getFreshTypeOfLiteralType): the regular and the fresh object of one literal; the value of a
   * number literal (its text is the source slice). Enum types and their literals use the same fields. */
  struct TsType *litRegular, *litFresh;
  double litNum;
  /* conditional types (ts-conditional.h): the root of the node, and the instantiated check and extends types */
  struct CondRoot *root;
  struct TsType *condCheck, *condExtends;
  struct TsMapper *condMapper, *condCombined;
  /* mapped types (getTypeFromMappedTypeNode): the type parameter of the node and the constraint type */
  struct TsType *mapTp, *mapConstraint;
  /* template literal types (ts-template.h): the texts between the spans; string mapping types: the mapped type */
  CStr *tlTexts;
  struct TsType *smType;
  /* array literal types: the clone of a type reference that carries ArrayLiteral (createArrayLiteralType) */
  struct TsType *literalType;
} TsType;

/* erased: the type parameters are instantiated to any (getErasedSignature, or an instantiation in context), so the
 * signature has none of its own */
struct CSig { TsNode *decl; bool isConstruct; TsType *thisType; struct TsMapper *mapper; bool erased; TsSymbol **psyms; TsSymbol *thisSym; TsType **retCell; };   /* psyms: the parameter symbols of this signature instance (instantiateSignature, cSigInstantiate; the non-this parameters in order); thisSym: its this parameter; retCell: the resolved return type of this instance (getReturnTypeOfSignature caches it on the signature), shared by the copies of the instance */
static void cSigInstantiate(CSig *dst, const CSig *src, struct TsMapper *m);   /* instantiateSignature parameters (below) */
/* getSignatureInstantiation: the instances of a declared generic signature, one per type argument list (cached) */
typedef struct CSigInst { TsNode *decl; TsType **args; int n; CSig sig; struct CSigInst *next; } CSigInst;
static CSigInst *cSigInstTab[1024];

/* createType counts (typeCount) */
static int cTypeCount;
/* createType counts of the string and number literal types (stringLiteralTypes and numberLiteralTypes creations, fresh copies included) */
static int cStringLitCount, cNumberLitCount;   /* all string and number literal types: created by cNewType */
static int cLitRegular[2], cLitFresh[2], cLitEnum[2];   /* [0] string, [1] number: regular (getStringLiteralType), fresh, enum members */
static int cNextTypeId = 1;
static bool cUnportedHit;          /* set when a type or relation outside the ported subset was met */
static int cUnportedCount;
static int cIdentityCacheCount;

static TsType *cNewType(uint32_t flags) {
  cTypeCount++;
  if (flags & TF_StringLiteral) cStringLitCount++;
  if (flags & TF_NumberLiteral) cNumberLitCount++;
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
  cStringLitCount = cNumberLitCount = 0;
  memset(cLitRegular, 0, sizeof cLitRegular);
  memset(cLitFresh, 0, sizeof cLitFresh);
  memset(cLitEnum, 0, sizeof cLitEnum);
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

enum { REL_Identity = 0, REL_Subtype, REL_StrictSubtype, REL_Assignable, REL_Comparable, REL_Count };
/* Ternary (False < Maybe < True, so And is min and Or is max) */
enum { RR_False = 0, RR_Maybe = 1, RR_True = 2 };
/* IntersectionState */
enum { IS_None = 0, IS_Source = 1, IS_Target = 2 };
/* VarianceFlags */
enum { VF_Covariant = 1, VF_Contravariant = 2, VF_Bivariant = 3, VF_Independent = 4 };

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
static TsType *cExprType(TsNode *e);                       /* ts-checker-body.h */
static TsType *cApparentOf(TsType *o);                    /* ts-checker-body.h */
static TsType *cWidenLiteralType(TsType *t);               /* ts-checker-body.h */
static TsType *cAddUndefined(TsType *t);                  /* ts-checker-body.h */
static TsType *cMutableLocationType(TsNode *e, TsType *t); /* ts-checker-body.h */
static TsType *cGetTypeFromConditionalTypeNode(TsNode *n);
static TsType *cGetTypeFromTemplateTypeNode(TsNode *n);   /* ts-template.h */
static bool cIsGenericIndex(TsType *t);
static TsType *cConstraintOf(TsType *tp);                   /* ts-relate.h */
static uint8_t *cVariances(TsType *tg);             /* ts-relate.h */
static bool cIsRelatedWith(TsType *s, TsType *t, int rel);   /* ts-relate.h */
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
/* the literal types by (flags, text), in a hash table of slots (cLitSlots holds index + 1 into cLitCache) */
static int32_t *cLitSlots;
static int cLitSlotCap;

static uint32_t cLitHash(uint32_t flags, const char *text, int n) {
  uint32_t h = 2166136261u ^ flags;
  for (int i = 0; i < n; i++) h = (h ^ (uint8_t)text[i]) * 16777619u;
  return h ^ (h >> 13);
}

static void cLitRehash(int cap) {
  free(cLitSlots);
  cLitSlots = calloc((size_t)cap, sizeof(int32_t));
  if (!cLitSlots) abort();
  cLitSlotCap = cap;
  for (int i = 0; i < cLitN; i++) {
    TsType *l = cLitCache[i];
    uint32_t mask = (uint32_t)cap - 1, h = cLitHash(l->flags, l->text, l->textLen) & mask;
    while (cLitSlots[h]) h = (h + 1) & mask;
    cLitSlots[h] = i + 1;
  }
}

static TsType *cLiteralType(uint32_t flags, const char *text, int n) {
  if ((cLitN + 1) * 2 > cLitSlotCap) cLitRehash(cLitSlotCap ? cLitSlotCap * 2 : 1024);
  uint32_t mask = (uint32_t)cLitSlotCap - 1, h = cLitHash(flags, text, n) & mask;
  for (;;) {
    int32_t v = cLitSlots[h];
    if (v == 0) break;
    TsType *l = cLitCache[v - 1];
    if (l->flags == flags && l->textLen == n && memcmp(l->text, text, (size_t)n) == 0) return l;
    h = (h + 1) & mask;
  }
  TsType *l = cNewType(flags);
  if (flags & TF_StringLiteral) cLitRegular[0]++;
  else if (flags & TF_NumberLiteral) cLitRegular[1]++;
  l->text = text;
  l->textLen = n;
  if (cLitN == cLitCap) {
    cLitCap = cLitCap ? cLitCap * 2 : 64;
    cLitCache = realloc(cLitCache, sizeof(TsType *) * (size_t)cLitCap);
    if (!cLitCache) abort();
  }
  cLitCache[cLitN++] = l;
  cLitSlots[h] = cLitN;
  return l;
}

/* getNumberLiteralType of a position index (a non-negative integer, keyed by its decimal text) */
static TsType *cIndexLiteral(int idx) {
  char buf[16];
  int n = snprintf(buf, sizeof buf, "%d", idx);
  char *text = ts_arena_alloc((size_t)n);
  memcpy(text, buf, (size_t)n);
  TsType *l = cLiteralType(TF_NumberLiteral, text, n);
  l->litNum = idx;
  return l;
}

/* getFreshTypeOfLiteralType: the fresh object of a regular literal, created once (a type of the checker) */
static TsType *cFreshLiteral(TsType *r) {
  if (r->litFresh) return r->litFresh;
  TsType *f = cNewType(r->flags);
  if (r->flags & TF_StringLiteral) cLitFresh[0]++;
  else if (r->flags & TF_NumberLiteral) cLitFresh[1]++;
  f->text = r->text;
  f->textLen = r->textLen;
  f->litNum = r->litNum;
  f->litRegular = r;
  f->litFresh = f;
  r->litRegular = r;
  r->litFresh = f;
  return f;
}

static bool cIsThislessInterface(TsSymbol *sym);

/* the declared type of an interface or class symbol (getDeclaredTypeOfClassOrInterface, 61868). A class, a
 * generic interface and an interface that mentions this are references to themselves with a this type
 * parameter (thisType, constraint: the declared type); the type arguments are the type parameters. */
static TsType *cDeclaredTypeOfClassOrInterface(TsSymbol *sym) {
  sym = cGetMergedSymbol(sym);
  if (sym->declaredType) return sym->declaredType;
  bool isClass = (sym->flags & SF_Class) != 0;
  TsType *t = cNewType(TF_Object);
  t->objectFlags = isClass ? OF_Class : OF_Interface;
  t->symbol = sym;
  sym->declaredType = t;
  /* the type parameters come from the first declaration (getOuterTypeParametersOfClassOrInterface + local ones) */
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
  if (ntp || isClass || !cIsThislessInterface(sym)) {
    t->objectFlags |= OF_Reference;
    t->target = t;
    t->args = t->typeParams;
    t->nargs = t->ntypeParams;
    t->instsInit = true;
    cInstAdd(t, t->typeParams, t->ntypeParams, t);   /* target.instantiations: the declared type itself */
    t->thisType = cNewType(TF_TypeParameter);
    t->thisType->symbol = sym;
    t->thisType->isThisType = true;
    t->thisType->tpConstraint = t;
    t->thisType->tpConstState = 2;
  }
  return t;
}

/* type aliases being resolved: a cycle through eagerly resolved positions is not in the ported subset
 * (TypeScript defers those references, or reports 2456) */
static TsSymbol *cAliasStack[64];
static int cAliasDepth;

static TsType *cDeclaredTypeOfTypeAlias(TsSymbol *sym) {
  if (sym->declaredType) return sym->declaredType;
  TsNode *d = sym->decls[0];
  /* a generic alias declares its body with its own type parameters (getDeclaredTypeOfTypeAlias); the
   * references instantiate that body (cGenericAliasReference) */
  for (int i = 0; i < cAliasDepth; i++) if (cAliasStack[i] == sym) return cUnported("circular type alias");
  if (cAliasDepth == 64) return cUnported("alias depth");
  cAliasStack[cAliasDepth++] = sym;
  TsType *t = cTypeFromNode(d->type);
  cAliasDepth--;
  /* getTypeFromUnionTypeNode: the union of an alias body has the alias (a union type of its own, not the interned union of
   * the same members). A generic alias keeps its type arguments, which are not ported. */
  if (t != cErrorType && (t->flags & TF_Union) && t->ntypes >= 2 && !(d->typeParameters && d->typeParameters->n)) {
    TsNode *b = d->type;
    while (b->kind == TSK_ParenthesizedType) b = b->type;
    if (b->kind == TSK_UnionType) {
      TsType *c = cNewType(TF_Union);
      c->types = t->types;
      c->ntypes = t->ntypes;
      c->objectFlags = t->objectFlags;
      c->aliasSymbol = sym;
      t = c;
    }
  }
  if (t == cIntrinsicMarkerType) {
    /* getDeclaredTypeOfTypeAlias (61914): BuiltinIteratorReturn is undefined under strictBuiltinIteratorReturn,
     * which the project's tsconfig gets from "strict": true */
    /* any other intrinsic alias keeps the marker as its declared type (node: type === intrinsicMarkerType) */
    if (bnEq(sym->name, bnC("BuiltinIteratorReturn"))) t = cUndefinedType;
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

/* enums: defined after cEntityNameSymbol (ts-types.h, "enums" below) */
static TsType *cEnumMemberType(TsSymbol *m);
static TsType *cDeclaredTypeOfEnum(TsSymbol *sym);
static TsType *cRegularType(TsType *t);

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
static int cStringMappingKind(TsSymbol *sym);   /* ts-template.h */
static TsType *cGenericAliasReference(TsSymbol *sym, TsNode *ref) {
  TsNode *ad = sym->decls[0];
  int ntp = ad->typeParameters->n;
  if (!sym->aliasHolder) {
    TsType *h = ts_arena_zalloc(sizeof(TsType));   /* the holder is not a type of the checker (not counted) */
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
  /* getTypeAliasInstantiation: an intrinsic alias (Uppercase, Lowercase, Capitalize, Uncapitalize) is the string
   * mapping of its argument, before the instantiation cache */
  TsType *body = cTypeFromNode(ad->type);
  if (body == cIntrinsicMarkerType) {
    if (ntp == 1 && n == 1 && cStringMappingKind(sym) >= 0) return cGetStringMappingType(sym, args[0]);
    return cUnported("intrinsic type alias");
  }
  TsInst *e = cInstFind(h, args, n);
  if (e) return e->val;
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
  if (s->flags & (SF_RegularEnum | SF_ConstEnum)) {
    /* getTypeReferenceType: tryGetDeclaredTypeOfSymbol, then checkNoTypeArguments */
    if (ref->typeArguments && ref->typeArguments->n) return cUnported("type arguments of an enum");
    return cRegularType(cDeclaredTypeOfEnum(s));
  }
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

/* ---- enums (getDeclaredTypeOfEnum 61932, computeEnumMemberValues, evaluate 23423) ----
 * An enum type is the union of the literal types of its members, one per distinct value (the regular
 * literal, with its fresh object for the member's declared type), and the members' values come from the
 * constant evaluator. A member without a value (computed, in an ambient enum) is an Enum type of its own. */
enum { EV_Undefined = 0, EV_Number = 1, EV_String = 2 };
typedef struct { int kind; double num; const char *s; int n; } CEnumVal;

static CEnumVal cEvalEnum(TsNode *e, TsNode *loc);

static CEnumVal cNoValue(void) {
  CEnumVal v = { EV_Undefined, 0, NULL, 0 };
  return v;
}

/* the value of an enum member (getEnumMemberValue): the declaration is an EnumMember node */
static CEnumVal cEnumMemberValue(TsNode *m);

/* the regular object of a literal type (getRegularTypeOfLiteralType); other types are their own */
static TsType *cRegularType(TsType *t) {
  if ((t->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral | TF_Enum)) && t->litRegular) return t->litRegular;
  return t;
}

static double cToInt32Num(double d) {
  if (!(d == d) || d - d != 0) return 0;   /* NaN and infinities */
  double m = fmod(trunc(d), 4294967296.0);
  if (m < 0) m += 4294967296.0;
  return m;
}

/* the numeric value of a numeric literal's source text (decimal, hex, octal and binary, with separators) */
static double cNumericLiteralValue(const char *t, int n) {
  char buf[128];
  int k = 0;
  for (int i = 0; i < n && k < (int)sizeof buf - 1; i++) if (t[i] != '_') buf[k++] = t[i];
  buf[k] = 0;
  if (k > 1 && buf[0] == '0' && (buf[1] == 'x' || buf[1] == 'X')) return (double)strtoull(buf + 2, NULL, 16);
  if (k > 1 && buf[0] == '0' && (buf[1] == 'o' || buf[1] == 'O')) return (double)strtoull(buf + 2, NULL, 8);
  if (k > 1 && buf[0] == '0' && (buf[1] == 'b' || buf[1] == 'B')) return (double)strtoull(buf + 2, NULL, 2);
  if (k > 1 && buf[0] == '0' && buf[1] >= '0' && buf[1] <= '9') { cUnported("legacy octal literal in an enum"); return 0; }
  return strtod(buf, NULL);
}

/* the value of an identifier or a property access that names an enum member, or a global number */
static CEnumVal cEvalEntityName(TsNode *e) {
  CEnumVal v = cNoValue();
  if (e->kind == TSK_PropertyAccessExpression) {
    /* E.M: the member of an enum (evaluateEntityNameExpression with the enum's exports) */
    if (!e->expression || e->expression->kind != TSK_Identifier || !e->name) return cUnported("enum initializer access"), v;
    TsSymbol *ls = cResolveNameMeaning(e, bnEscapeText(e->expression->text, e->expression->textLen), SF_Value);
    if (!ls) return v;
    if (ls->flags & SF_Alias) return cUnported("enum initializer through an alias"), v;
    ls = cGetMergedSymbol(ls);
    if (!(ls->flags & SF_Enum) || !ls->exports) return v;
    TsSymbol *m = bTableGet(ls->exports, bnEscapeText(e->name->text, e->name->textLen));
    if (!m || !(m->flags & SF_EnumMember)) return v;
    return cEnumMemberValue(m->decls[0]);
  }
  /* identifier */
  BName name = bnEscapeText(e->text, e->textLen);
  TsSymbol *s = cResolveNameMeaning(e, name, SF_Value);
  if (!s) return v;
  if (s->flags & SF_Alias) return cUnported("enum initializer through an alias"), v;
  s = cGetMergedSymbol(s);
  if ((name.n == 8 && memcmp(name.s, "Infinity", 8) == 0) || (name.n == 3 && memcmp(name.s, "NaN", 3) == 0)) {
    TsSymbol *g = bTableGet(cGlobals, name);
    if (g == s) {
      v.kind = EV_Number;
      v.num = name.n == 3 ? NAN : INFINITY;
      return v;
    }
  }
  if (s->flags & SF_EnumMember) return cEnumMemberValue(s->decls[0]);
  if (s->flags & SF_Variable) return cUnported("enum initializer from a variable"), v;
  return v;
}

/* evaluate (23423): numbers, strings, enum members by name, and the operators of constant expressions */
static CEnumVal cEvalEnum(TsNode *e, TsNode *loc) {
  (void)loc;
  CEnumVal v = cNoValue();
  while (e->kind == TSK_ParenthesizedExpression) e = e->expression;
  switch (e->kind) {
    case TSK_PrefixUnaryExpression: {
      CEnumVal a = cEvalEnum(e->operand, loc);
      if (a.kind != EV_Number) return v;
      if (e->extra == TSK_PlusToken) return a;
      if (e->extra == TSK_MinusToken) { a.num = -a.num; return a; }
      if (e->extra == TSK_TildeToken) { a.num = (double)~(int32_t)(uint32_t)cToInt32Num(a.num); return a; }
      return v;
    }
    case TSK_BinaryExpression: {
      CEnumVal a = cEvalEnum(e->left, loc);
      CEnumVal b = cEvalEnum(e->right, loc);
      int op = e->operatorToken->kind;
      if (a.kind == EV_Number && b.kind == EV_Number) {
        uint32_t ua = (uint32_t)cToInt32Num(a.num), ub = (uint32_t)cToInt32Num(b.num);
        int32_t sa = (int32_t)ua;
        v.kind = EV_Number;
        switch (op) {
          case TSK_BarToken: v.num = (double)(int32_t)(ua | ub); return v;
          case TSK_AmpersandToken: v.num = (double)(int32_t)(ua & ub); return v;
          case TSK_CaretToken: v.num = (double)(int32_t)(ua ^ ub); return v;
          case TSK_GreaterThanGreaterThanToken: v.num = (double)(sa >> (ub & 31)); return v;
          case TSK_GreaterThanGreaterThanGreaterThanToken: v.num = (double)(ua >> (ub & 31)); return v;
          case TSK_LessThanLessThanToken: v.num = (double)(int32_t)(ua << (ub & 31)); return v;
          case TSK_AsteriskToken: v.num = a.num * b.num; return v;
          case TSK_SlashToken: v.num = a.num / b.num; return v;
          case TSK_PlusToken: v.num = a.num + b.num; return v;
          case TSK_MinusToken: v.num = a.num - b.num; return v;
          case TSK_PercentToken: v.num = fmod(a.num, b.num); return v;
          case TSK_AsteriskAsteriskToken: v.num = pow(a.num, b.num); return v;
          default: return cNoValue();
        }
      }
      if (op == TSK_PlusToken && a.kind != EV_Undefined && b.kind != EV_Undefined && (a.kind == EV_String || b.kind == EV_String)) {
        /* string concatenation; a number part must print the way JavaScript prints an integer */
        char nb[64];
        const char *ps[2];
        int pn[2];
        CEnumVal *parts[2] = { &a, &b };
        for (int i = 0; i < 2; i++) {
          if (parts[i]->kind == EV_String) { ps[i] = parts[i]->s; pn[i] = parts[i]->n; continue; }
          double d = parts[i]->num;
          if (d != floor(d) || fabs(d) >= 1e15) return cUnported("enum string concatenation of a number"), v;
          pn[i] = snprintf(nb, sizeof nb, "%.0f", d);
          if (i == 0) { char *c = ts_arena_alloc((size_t)pn[i] + 1); memcpy(c, nb, (size_t)pn[i] + 1); ps[i] = c; }
          else ps[i] = nb;
        }
        int total = pn[0] + pn[1];
        char *out = ts_arena_alloc((size_t)total + 1);
        memcpy(out, ps[0], (size_t)pn[0]);
        memcpy(out + pn[0], ps[1], (size_t)pn[1]);
        out[total] = 0;
        v.kind = EV_String;
        v.s = out;
        v.n = total;
        return v;
      }
      return v;
    }
    case TSK_StringLiteral: case TSK_NoSubstitutionTemplateLiteral:
      if (memchr(e->text, '\\', (size_t)e->textLen)) return cUnported("enum string with an escape"), v;
      v.kind = EV_String;
      v.s = e->text;
      v.n = e->textLen;
      return v;
    case TSK_NumericLiteral:
      v.kind = EV_Number;
      v.num = cNumericLiteralValue(e->text, e->textLen);
      return v;
    case TSK_Identifier: case TSK_PropertyAccessExpression:
      return cEvalEntityName(e);
    default:
      return v;
  }
}

/* computeConstantEnumMemberValue / computeEnumMemberValue (member with no initializer: the next value) */
static CEnumVal cComputeEnumMemberValue(TsNode *m, bool hasAuto, double autoValue) {
  if (m->name && m->name->kind == TSK_ComputedPropertyName) return cUnported("computed enum member name"), cNoValue();
  if (m->initializer) {
    CEnumVal r = cEvalEnum(m->initializer, m);
    /* an initializer that is not a constant is checked as an expression, which is not ported */
    if (r.kind == EV_Undefined) return cUnported("enum member initializer is not a constant"), r;
    return r;
  }
  if ((m->parent->flags & TNF_Ambient) && !bIsEnumConst(m->parent)) return cNoValue();
  if (!hasAuto) return cUnported("enum member without an initializer after a computed value"), cNoValue();
  CEnumVal r = { EV_Number, autoValue, NULL, 0 };
  return r;
}

/* computeEnumMemberValues (89964): the values of all members of one enum declaration, in order */
static void cComputeEnumMemberValues(TsNode *ed) {
  if (ed->enumValuesComputed) return;
  ed->enumValuesComputed = 1;
  bool hasAuto = true;
  double autoValue = 0;
  if (!ed->members) return;
  for (int i = 0; i < ed->members->n; i++) {
    TsNode *m = ed->members->v[i];
    CEnumVal r = cComputeEnumMemberValue(m, hasAuto, autoValue);
    m->enumValueDone = 1;
    m->enumValueKind = (uint8_t)r.kind;
    m->enumValueNum = r.num;
    m->enumValueStr = r.s;
    m->enumValueLen = r.n;
    hasAuto = r.kind == EV_Number;   /* autoValue = typeof value === "number" ? value + 1 : undefined */
    autoValue = r.num + 1;
  }
}

static CEnumVal cEnumMemberValue(TsNode *m) {
  if (!m->parent || m->parent->kind != TSK_EnumDeclaration) return cUnported("enum member without its enum"), cNoValue();
  cComputeEnumMemberValues(m->parent);
  CEnumVal v = cNoValue();
  if (m->enumValueDone) {
    v.kind = m->enumValueKind;
    v.num = m->enumValueNum;
    v.s = m->enumValueStr;
    v.n = m->enumValueLen;
  } else {
    /* a reference to a member whose value is still being computed: a forward reference (not ported) */
    cUnported("enum forward reference");
  }
  return v;
}

/* a regular and a fresh object of a computed enum member (createComputedEnumType) */
static TsType *cCreateComputedEnumType(TsSymbol *sym) {
  TsType *reg = cNewType(TF_Enum);
  TsType *fr = cNewType(TF_Enum);
  reg->symbol = sym;
  fr->symbol = sym;
  reg->litRegular = reg;
  reg->litFresh = fr;
  fr->litRegular = reg;
  fr->litFresh = fr;
  return reg;
}

/* getDeclaredTypeOfEnum: the declared type of the enum symbol (all its declarations) */
static TsType *cDeclaredTypeOfEnum(TsSymbol *sym) {
  sym = cGetMergedSymbol(sym);
  if (sym->declaredType) return sym->declaredType;
  int nm = 0, nl = 0;
  int cap = 0;
  for (int d = 0; d < sym->ndecls; d++)
    if (sym->decls[d]->kind == TSK_EnumDeclaration && sym->decls[d]->members) cap += sym->decls[d]->members->n;
  TsType **mlist = ts_arena_alloc(sizeof(TsType *) * (size_t)(cap ? cap : 1));
  TsType **lits = ts_arena_alloc(sizeof(TsType *) * (size_t)(cap ? cap : 1));   /* one regular literal per value */
  CEnumVal *lvals = ts_arena_alloc(sizeof(CEnumVal) * (size_t)(cap ? cap : 1));
  for (int d = 0; d < sym->ndecls; d++) {
    TsNode *ed = sym->decls[d];
    if (ed->kind != TSK_EnumDeclaration || !ed->members) continue;
    for (int i = 0; i < ed->members->n; i++) {
      TsNode *m = ed->members->v[i];
      TsSymbol *ms = cGetMergedSymbol(m->symbol);
      CEnumVal v = cEnumMemberValue(m);
      TsType *reg, *fresh;
      if (v.kind == EV_Undefined) {
        reg = cCreateComputedEnumType(ms);
        fresh = reg->litFresh;
      } else {
        int k = 0;
        while (k < nl && !(lvals[k].kind == v.kind && (v.kind == EV_Number ? lvals[k].num == v.num
                                                       : lvals[k].n == v.n && memcmp(lvals[k].s, v.s, (size_t)v.n) == 0)))
          k++;
        if (k < nl) {
          reg = lits[k];
          fresh = reg->litFresh;
        } else {
          /* getEnumLiteralType (created once per value and enum) and getFreshTypeOfLiteralType */
          uint32_t fl = TF_EnumLiteral | (v.kind == EV_String ? TF_StringLiteral : TF_NumberLiteral);
          reg = cNewType(fl);
          fresh = cNewType(fl);
          cLitEnum[v.kind == EV_String ? 0 : 1] += 2;
          reg->symbol = ms;
          fresh->symbol = ms;
          reg->litNum = v.num;
          reg->text = v.s;
          reg->textLen = v.n;
          fresh->litNum = v.num;
          fresh->text = v.s;
          fresh->textLen = v.n;
          reg->litRegular = reg;
          reg->litFresh = fresh;
          fresh->litRegular = reg;
          fresh->litFresh = fresh;
          lits[nl] = reg;
          lvals[nl] = v;
          nl++;
        }
      }
      ms->declaredType = fresh;   /* getSymbolLinks(memberSymbol).declaredType */
      mlist[nm++] = reg;          /* getRegularTypeOfLiteralType(memberType) */
    }
  }
  TsType *et = nm ? cGetUnionType(mlist, nm) : cCreateComputedEnumType(sym);
  if (et->flags & TF_Union) {
    et->flags |= TF_EnumLiteral;
    et->symbol = sym;
  }
  sym->declaredType = et;
  return et;
}

/* the type of an enum member used as a type: getDeclaredTypeOfEnumMember, regular (getTypeReferenceType) */
static TsType *cEnumMemberType(TsSymbol *m) {
  m = cGetMergedSymbol(m);
  if (!m->declaredType) {
    if (!m->decls[0]->parent || !m->decls[0]->parent->symbol) return cUnported("enum member without its enum");
    cDeclaredTypeOfEnum(m->decls[0]->parent->symbol);
  }
  if (!m->declaredType) return cUnported("enum member type");
  return cRegularType(m->declaredType);
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

/* globalReadonlyArrayType: the generic ReadonlyArray interface of the global scripts */
static TsType *cGlobalReadonlyArrayType;
static TsType *cGlobalReadonlyArrayDeclared(void) {
  if (!cGlobalReadonlyArrayType) {
    TsSymbol *a = bTableGet(cGlobals, bnC("ReadonlyArray"));
    if (!a) abort();
    cGlobalReadonlyArrayType = cDeclaredTypeOfClassOrInterface(a);
  }
  return cGlobalReadonlyArrayType;
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
static TsType *cTupleReference(TsType **elems, int n, bool readonly);

/* getTypeFromTypeNode (67684): the type of a type node, cached on the node (links.resolvedType) */
static TsType *cTypeFromNode(TsNode *n) {
  if (!n) return cErrorType;
  if (n->resolvedType) return n->resolvedType;
  TsType *t = cTypeFromNodeWorker(n);
  n->resolvedType = t;
  return t;
}

/* getThisContainer (not through arrow functions, class computed names or decorators) */
static TsNode *cThisContainer(TsNode *n) {
  for (TsNode *p = n->parent; p; p = p->parent) {
    switch (p->kind) {
      case TSK_ComputedPropertyName: p = p->parent->parent; break;   /* node = node.parent.parent */
      case TSK_FunctionDeclaration: case TSK_FunctionExpression: case TSK_ModuleDeclaration: case TSK_ClassStaticBlockDeclaration:
      case TSK_PropertyDeclaration: case TSK_PropertySignature: case TSK_MethodDeclaration: case TSK_MethodSignature:
      case TSK_Constructor: case TSK_GetAccessor: case TSK_SetAccessor: case TSK_CallSignature: case TSK_ConstructSignature:
      case TSK_IndexSignature: case TSK_EnumDeclaration: case TSK_SourceFile:
        return p;
      default: break;
    }
  }
  return NULL;
}

/* getThisType (67621): the this type of the class or interface that contains a non-static member (or a constructor's
 * body); other this types are an error in TypeScript and are not ported */
static TsType *cThisTypeOfNode(TsNode *n) {
  TsNode *c = cThisContainer(n);
  TsNode *owner = c ? c->parent : NULL;
  if (owner && (owner->kind == TSK_ClassDeclaration || owner->kind == TSK_ClassExpression || owner->kind == TSK_InterfaceDeclaration) &&
      owner->symbol) {
    bool isStatic = c->kind == TSK_ClassStaticBlockDeclaration || (bModifierFlags(c) & TMF_Static) != 0;
    bool inBody = false;
    if (c->kind == TSK_Constructor && c->body)
      for (TsNode *x = n; x && !inBody; x = x->parent) inBody = x == c->body;
    if (!isStatic && (c->kind != TSK_Constructor || inBody))
      return cDeclaredTypeOfClassOrInterface(cGetMergedSymbol(owner->symbol))->thisType;
  }
  return cUnported("this type outside a class or interface member");
}

/* ---- indexed access and index types (getIndexedAccessType 67063, getIndexType 64940) ---- */
static void cResolveIndexInfos(TsType *t);
static int cPropertiesOf(TsType *t, TsSymbol ***out);
static TsSymbol *cGetPropertyOfUnionOrIntersection(TsType *t, BName name);   /* union properties (before cTypeOfSymbol) */
static int cPropertiesOfUnionOrIntersection(TsType *t, TsSymbol ***out);
static BName cLiteralKey(TsType *l);
static TsSymbol *cPropertyOf(TsType *t, BName name);
static CHEnt *cIndexedTab[1024];

/* a string literal type of a value (the token value of a literal node, without quotes; literal types are keyed by it) */
static TsType *cStringLiteralType(const char *value, int n) { return cLiteralType(TF_StringLiteral, value, n); }

/* isGenericObjectType (type variables, indexed access types; a generic mapped type is not ported) */
static bool cIsGenericObject(TsType *t) {
  if (t->flags & (TF_Union | TF_Intersection)) {
    for (int i = 0; i < t->ntypes; i++) if (cIsGenericObject(t->types[i])) return true;
    return false;
  }
  if (t->flags & TF_Object) {
    /* isGenericMappedType: a generic index type as the constraint */
    if (t->objectFlags & OF_Mapped) return cIsGenericIndex(t->mapConstraint);
    return false;
  }
  return (t->flags & (TF_TypeParameter | TF_IndexedAccess | TF_Conditional)) != 0;
}

/* isGenericIndexType: type variables, index types, template literal and string mapping types (not patterns), and unions and intersections of them */
static bool cIsPatternLiteralType(TsType *t);
static bool cIsGenericIndex(TsType *t) {
  if (t->flags & (TF_Union | TF_Intersection)) {
    for (int i = 0; i < t->ntypes; i++) if (cIsGenericIndex(t->types[i])) return true;
    return false;
  }
  if (t->flags & (TF_TemplateLiteral | TF_StringMapping)) return !cIsPatternLiteralType(t);
  return (t->flags & (TF_TypeParameter | TF_IndexedAccess | TF_Index | TF_Conditional)) != 0;
}

/* ts-conditional.h: conditional types (after the generic checks they use) */
#include "ts-conditional.h"

/* ts-template.h: template literal and string mapping types (after the conditional types they use) */
#include "ts-template.h"

/* getIndexTypeForGenericType: the deferred keyof of a type variable, one per type */
static TsType *cIndexTypeForGeneric(TsType *t) {
  if (!t->resolvedIndex) {
    TsType *r = cNewType(TF_Index);
    r->ixOf = t;
    t->resolvedIndex = r;
  }
  return t->resolvedIndex;
}

/* the applicable index info of a key (getApplicableIndexInfo, a single one; several are not ported) */
static CIndexInfo *cApplicableIndexInfo(TsType *o, TsType *key, bool *unported) {
  cResolveIndexInfos(o);
  CIndexInfo *hit = NULL;
  int n = 0;
  for (int i = 0; i < o->nindex; i++) {
    CIndexInfo *info = &o->index[i];
    /* isApplicableIndexType: a string index takes any string or number key, a number index a number key */
    bool applies = info->keyType == cStringType ? (key->flags & (TF_StringLiteral | TF_String | TF_NumberLiteral | TF_Number)) != 0
                                                : (info->keyType == cNumberType && (key->flags & (TF_NumberLiteral | TF_Number)) != 0);
    if (applies) { hit = info; n++; }
  }
  if (n > 1) *unported = true;
  return hit;
}

/* getPropertyTypeForIndexType (for the keys of the ported subset): the property named by a literal key, or
 * the applicable index signature; NULL when the access is an error in TypeScript (not ported) */
static TsType *cPropertyTypeForIndex(TsType *o, TsType *key) {
  if (o->flags & (TF_Any | TF_Never)) return (key->flags & (TF_StringLiteral | TF_NumberLiteral | TF_String | TF_Number)) ? o : NULL;
  if (!(o->flags & TF_Object)) return NULL;
  if (key->flags & (TF_StringLiteral | TF_NumberLiteral)) {
    BName name = (key->flags & TF_StringLiteral) ? cLiteralKey(key) : bnEscapeText(key->text, key->textLen);
    TsSymbol *p = cPropertyOf(o, name);
    if (p) return cTypeOfSymbol(p);
  }
  if (!(key->flags & (TF_StringLiteral | TF_NumberLiteral | TF_String | TF_Number))) return NULL;
  bool unported = false;
  CIndexInfo *info = cApplicableIndexInfo(o, key, &unported);
  if (unported) { cUnported("several applicable index signatures"); return NULL; }
  return info ? info->type : NULL;
}

/* getIndexedAccessType (67063) for the resolved forms. A missing property is an error at an access node (a type node:
 * not ported) and unknownType without one (an instantiation of T[K], as TypeScript does for K = string). */
static TsType *cGetIndexedAccessType(TsType *o, TsType *ix, bool accessNode) {
  if (o == cErrorType || ix == cErrorType) return cErrorType;
  if (cIsGenericIndex(ix) || cIsGenericObject(o)) {
    if (o->flags & (TF_Any | TF_Unknown)) return o;
    TsType *key[2] = {o, ix};
    TsType *hit = cHashGet(cIndexedTab, key, 2);
    if (hit) return hit;
    TsType *r = cNewType(TF_IndexedAccess);
    r->iaObj = o;
    r->iaIdx = ix;
    TsType **k = ts_arena_alloc(sizeof(TsType *) * 2);
    k[0] = o; k[1] = ix;
    cHashPut(cIndexedTab, k, 2, r);
    return r;
  }
  if (ix->flags & TF_Union && !(ix->flags & TF_Boolean)) {
    TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(ix->ntypes ? ix->ntypes : 1));
    for (int i = 0; i < ix->ntypes; i++) {
      parts[i] = cPropertyTypeForIndex(o, ix->types[i]);
      if (!parts[i]) return accessNode ? cUnported("indexed access: missing property of a union key") : cUnknownType;
    }
    return cGetUnionType(parts, ix->ntypes);
  }
  TsType *r = cPropertyTypeForIndex(o, ix);
  if (r) return r;
  if (!accessNode && (o->flags & TF_Object) && (ix->flags & (TF_StringLiteral | TF_NumberLiteral | TF_String | TF_Number | TF_Any)))
    return cUnknownType;   /* no property and no index signature: getIndexedAccessType without an access node */
  return cUnported("indexed access: no such property or index signature");
}

/* getLiteralTypeFromProperties (for the object types of the port): the names of the public properties as
 * literal types, then the key types of the index signatures (a string index gives string | number) */
static TsType *cLateNameType(TsNode *member);   /* below: the unique symbol type of a late-bound member name */
static TsType *cLiteralKeysOf(TsType *t) {
  TsSymbol **props;
  int np = cPropertiesOf(t, &props);
  TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(np + 4));
  int n = 0;
  for (int i = 0; i < np; i++) {
    TsSymbol *p = props[i];
    TsNode *d = p->valueDeclaration ? p->valueDeclaration : (p->ndecls ? p->decls[0] : NULL);
    if (d && (bModifierFlags(d) & (TMF_Private | TMF_Protected))) continue;   /* NonPublicAccessibilityModifier */
    if (p->name.n >= 3 && memcmp(p->name.s, "__@", 3) == 0) {
      /* getLiteralTypeFromProperty: the name type set by late binding, a unique symbol (a literal name is early) */
      TsType *lt = (d && d->name && d->name->kind == TSK_ComputedPropertyName) ? cLateNameType(d) : NULL;
      if (!lt) { cUnported("keyof of a late-bound name"); continue; }
      if (lt != cErrorType) parts[n++] = lt;
      continue;
    }
    if (!d || !d->name) { parts[n++] = cStringLiteralType(p->name.s, p->name.n); continue; }
    TsNode *nm = d->name;
    if (nm->kind == TSK_NumericLiteral) parts[n++] = cLiteralType(TF_NumberLiteral, nm->text, nm->textLen);
    else if (nm->kind == TSK_Identifier || nm->kind == TSK_StringLiteral) parts[n++] = cStringLiteralType(nm->text, nm->textLen);
    else cUnported("keyof of a computed property name");
  }
  cResolveIndexInfos(t);
  for (int i = 0; i < t->nindex; i++) {
    TsType *k = t->index[i].keyType;
    if (k == cStringType) parts[n++] = cGetUnionType((TsType*[]){cStringType, cNumberType}, 2);
    else if (k == cNumberType) parts[n++] = cNumberType;
    else cUnported("keyof of a symbol index signature");
  }
  return cGetUnionType(parts, n);
}

/* getIndexType (64940) for the object types and type variables of the port */
static TsType *cGetIndexType(TsType *t) {
  if (t == cErrorType) return cErrorType;
  if (t->flags & (TF_TypeParameter | TF_IndexedAccess)) return cIndexTypeForGeneric(t);
  if (t->flags & TF_Union) {
    TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(t->ntypes ? t->ntypes : 1));
    for (int i = 0; i < t->ntypes; i++) parts[i] = cGetIndexType(t->types[i]);
    return cGetIntersectionType(parts, t->ntypes);
  }
  if (t->flags & TF_Intersection) {
    TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(t->ntypes ? t->ntypes : 1));
    for (int i = 0; i < t->ntypes; i++) parts[i] = cGetIndexType(t->types[i]);
    return cGetUnionType(parts, t->ntypes);
  }
  if (t->flags & TF_Object) {
    if (t->objectFlags & OF_Mapped) return cUnported("keyof of a mapped type");
    return cLiteralKeysOf(t);
  }
  if (t->flags & (TF_Any | TF_Never)) {
    TsType *parts[3] = {cStringType, cNumberType, cESSymbolType};
    return cGetUnionType(parts, 3);
  }
  if (t->flags & TF_Unknown) return cNeverType;
  return cUnported("keyof of a primitive or index type");
}

static TsType *cTypeFromNodeWorker(TsNode *n) {
  if (!n) return cErrorType;
  switch (n->kind) {
    case TSK_ThisType: return cThisTypeOfNode(n);
    case TSK_IndexedAccessType: {
      /* getTypeFromIndexedAccessTypeNode: the object and index types, then getIndexedAccessType */
      TsType *o = cTypeFromNode(n->objectType);
      TsType *ix = cTypeFromNode(n->indexType);
      if (o == cErrorType || ix == cErrorType) return cErrorType;
      return cGetIndexedAccessType(o, ix, true);
    }
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
    case TSK_TypePredicate: return n->assertsModifier ? cVoidType : cBooleanType; /* getTypeFromTypeNodeWorker */
    case TSK_TypeOperator:
      if (n->extra == TSK_KeyOfKeyword) {
        /* getTypeFromTypeOperatorNode: keyof T is getIndexType of the operand */
        TsType *o = cTypeFromNode(n->type);
        return o == cErrorType ? cErrorType : cGetIndexType(o);
      }
      if (n->extra == TSK_ReadonlyKeyword) {
        /* getTypeFromTypeOperatorNode: readonly T[] is ReadonlyArray<T> (the array's element type, referenced
         * without deferral as the Array case does); readonly tuples are not ported */
        if (n->type && n->type->kind == TSK_ArrayType) {
          TsType *el = cTypeFromNode(n->type->elementType);
          if (el == cErrorType) return cErrorType;
          return cTypeReference(cGlobalReadonlyArrayDeclared(), &el, 1);
        }
        if (!n->type) return cUnported("readonly operator without an operand");
        return cTypeFromNode(n->type);
      }
      if (n->extra != TSK_UniqueKeyword) return cUnported("type operator");
      if (!n->type || n->type->kind != TSK_SymbolKeyword) return cUnported("unique operand");
      return cUniqueSymbolType(n->parent);
    case TSK_LiteralType: {
      TsNode *l = n->literal;
      if (!l) return cUnported("literal type");
      if (l->kind == TSK_NullKeyword) return cNullType;
      if (l->kind == TSK_TrueKeyword) return cTrueType;
      if (l->kind == TSK_FalseKeyword) return cFalseType;
      /* getTypeFromLiteralTypeNode: checkExpression of the literal, whose fresh type is created with it */
      if (l->kind == TSK_StringLiteral || l->kind == TSK_NumericLiteral) {
        uint32_t fl = l->kind == TSK_StringLiteral ? TF_StringLiteral : TF_NumberLiteral;
        TsType *r = cLiteralType(fl, l->text, l->textLen);
        if (fl == TF_NumberLiteral) r->litNum = cNumericLiteralValue(l->text, l->textLen);
        cFreshLiteral(r);
        return r;
      }
      /* a negative numeric literal (checkPrefixUnaryExpression: the fresh literal of -value, one object) */
      if (l->kind == TSK_PrefixUnaryExpression && l->extra == TSK_MinusToken && l->operand && l->operand->kind == TSK_NumericLiteral) {
        const char *digits = l->operand->text;
        int dn = l->operand->textLen;
        if (cHasEscapeOrNonAscii(digits, dn) || memchr(digits, '_', (size_t)dn)) return cUnported("negative literal type value");
        TsType *r = cLiteralType(TF_NumberLiteral, cArenaCat("-", 1, digits, dn), dn + 1);
        r->litNum = -cNumericLiteralValue(digits, dn);
        cFreshLiteral(r);
        return r;
      }
      /* a bigint literal (getBigIntLiteralType of the base 10 digits; decimal digits without separators only) */
      if (l->kind == TSK_BigIntLiteral) {
        int bn = l->textLen;
        if (bn < 2 || l->text[bn - 1] != 'n' || (bn > 2 && l->text[0] == '0') || memchr(l->text, '_', (size_t)bn)) return cUnported("bigint literal type value");
        for (int i = 0; i < bn - 1; i++) if (l->text[i] < '0' || l->text[i] > '9') return cUnported("bigint literal type value");
        TsType *r = cLiteralType(TF_BigIntLiteral, l->text, bn - 1);
        cFreshLiteral(r);
        return r;
      }
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
    case TSK_ConditionalType: return cGetTypeFromConditionalTypeNode(n);
    case TSK_TemplateLiteralType: return cGetTypeFromTemplateTypeNode(n);
    case TSK_InferType: return cTypeOfTypeParameter(cGetMergedSymbol(n->typeParameter->symbol));   /* getTypeFromInferTypeNode */
    case TSK_MappedType: {
      /* getTypeFromMappedTypeNode: the object, its type parameter, then the constraint (eagerly, as TypeScript does) */
      TsType *t = cNewType(TF_Object);
      t->objectFlags = OF_Mapped;
      t->symbol = n->symbol;
      t->typeNode = n;
      n->resolvedType = t;
      TsType *P = cTypeOfTypeParameter(cGetMergedSymbol(n->typeParameter->symbol));
      t->mapTp = P;
      /* getConstraintOfTypeParameter: hasNonCircularBaseConstraint walks the chain of type parameter constraints */
      for (TsType *b = P, *g = NULL; b && (b->flags & TF_TypeParameter) && !b->isThisType && b != g; b = cConstraintOf(b)) g = b;
      TsType *c = cConstraintOf(P);
      t->mapConstraint = c ? c : cErrorType;
      return t;
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
      /* a tuple under the readonly type operator is readonly (getTypeFromArrayOrTupleTypeNode) */
      bool readonly = n->parent && n->parent->kind == TSK_TypeOperator && n->parent->extra == TSK_ReadonlyKeyword;
      return cTupleReference(el, cnt, readonly);
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
      /* checkQualifiedName of a value on the left (checkPropertyAccessExpressionOrQualifiedName): the property's type, regular */
      if (e && e->kind == TSK_QualifiedName && e->left && e->left->kind == TSK_Identifier && e->right && e->right->kind == TSK_Identifier) {
        TsType *lt = cExprType(e->left);
        if (lt == cErrorType) return lt;
        TsType *o = cApparentOf(lt);
        if (o == cErrorType || !(o->flags & TF_Object)) return cUnported("type query");
        TsSymbol *ps = cPropertyOf(o, bnEscapeText(e->right->text, e->right->textLen));
        if (!ps) return cUnported("type query");
        return cRegularType(cTypeOfSymbol(ps));
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
static TsType *cTupleTargets[2][16];
static TsType *cTupleTarget(int arity, bool readonly) {
  if (arity < 16 && cTupleTargets[readonly][arity]) return cTupleTargets[readonly][arity];
  TsType *tg = cNewType(TF_Object);
  tg->objectFlags = OF_Tuple | OF_Interface | (readonly ? OF_ReadonlyTuple : 0);
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
  tg->bases[0] = cTypeReference(readonly ? cGlobalReadonlyArrayDeclared() : cGlobalArrayDeclared(), &elem, 1);
  tg->nbases = 1;
  tg->basesDone = true;
  if (arity < 16) cTupleTargets[readonly][arity] = tg;
  return tg;
}

static TsType *cTupleReference(TsType **elems, int n, bool readonly) {
  TsType *tg = cTupleTarget(n, readonly);
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

/* isThislessType (61895): a type node that does not mention this (keywords, literals, arrays and references of such) */
static bool cIsThislessTypeNode(TsNode *n) {
  switch (n->kind) {
    case TSK_AnyKeyword: case TSK_UnknownKeyword: case TSK_StringKeyword: case TSK_NumberKeyword: case TSK_BigIntKeyword:
    case TSK_BooleanKeyword: case TSK_SymbolKeyword: case TSK_ObjectKeyword: case TSK_VoidKeyword:
    case TSK_UndefinedKeyword: case TSK_NeverKeyword: case TSK_LiteralType:
      return true;
    case TSK_ArrayType: return cIsThislessTypeNode(n->elementType);
    case TSK_TypeReference:
      for (int i = 0; n->typeArguments && i < n->typeArguments->n; i++)
        if (!cIsThislessTypeNode(n->typeArguments->v[i])) return false;
      return true;
    default: return false;
  }
}

static bool cIsThislessTypeParameterNode(TsNode *tp) { return !tp->constraint || cIsThislessTypeNode(tp->constraint); }

/* isThislessVariableLikeDeclaration (61900) */
static bool cIsThislessVariableLike(TsNode *d) {
  if (d->type) return cIsThislessTypeNode(d->type);
  return !d->initializer;
}

/* isThislessFunctionLikeDeclaration (61906) */
static bool cIsThislessFunctionLike(TsNode *d) {
  bool ok = d->kind == TSK_Constructor || (d->type && cIsThislessTypeNode(d->type));
  if (!ok) return false;
  for (int i = 0; d->parameters && i < d->parameters->n; i++)
    if (!cIsThislessVariableLike(d->parameters->v[i])) return false;
  for (int i = 0; d->typeParameters && i < d->typeParameters->n; i++)
    if (!cIsThislessTypeParameterNode(d->typeParameters->v[i])) return false;
  return true;
}

/* isThisless (62054): a member whose declaration does not mention this (its type needs no instantiation) */
static bool cIsThisless(TsSymbol *s) {
  if (s->ndecls != 1) return false;
  TsNode *d = s->decls[0];
  switch (d->kind) {
    case TSK_PropertyDeclaration: case TSK_PropertySignature: return cIsThislessVariableLike(d);
    case TSK_MethodDeclaration: case TSK_MethodSignature: case TSK_Constructor: case TSK_GetAccessor: case TSK_SetAccessor:
      return cIsThislessFunctionLike(d);
    default: return false;
  }
}

/* isThislessInterface (61839): an interface without this in its declarations and with no base that has a this type */
static bool cIsThislessInterface(TsSymbol *sym) {
  for (int d = 0; d < sym->ndecls; d++) {
    TsNode *dn = sym->decls[d];
    if (dn->kind != TSK_InterfaceDeclaration) continue;
    if (dn->flags & TNF_ContainsThis) return false;
    for (int h = 0; dn->heritageClauses && h < dn->heritageClauses->n; h++) {
      TsNode *hc = dn->heritageClauses->v[h];
      if (hc->extra != TSK_ExtendsKeyword) continue;
      for (int e = 0; hc->types && e < hc->types->n; e++) {
        TsNode *expr = hc->types->v[e]->expression;
        if (!expr || (expr->kind != TSK_Identifier && expr->kind != TSK_PropertyAccessExpression)) continue;
        TsSymbol *bs = cBaseSymbol(dn, expr);
        if (!bs || !(bs->flags & SF_Interface) || cDeclaredTypeOfClassOrInterface(bs)->thisType) return false;
      }
    }
  }
  return true;
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

/* getTypeWithThisArgument (62278): a reference gets the this argument as its last type argument (a
 * reference to a target without a this type, such as a tuple, is returned as it is) */
static TsType *cGetTypeWithThisArgument(TsType *t, TsType *thisArg) {
  if (t->flags & TF_Object) {
    if (!(t->objectFlags & OF_Reference)) return t;
    TsType *tg = t->target;
    if (!tg->thisType || tg->ntypeParams != t->nargs) return t;
    TsType **args = ts_arena_alloc(sizeof(TsType *) * (size_t)(t->nargs + 1));
    if (t->nargs) memcpy(args, t->args, sizeof(TsType *) * (size_t)t->nargs);
    args[t->nargs] = thisArg ? thisArg : tg->thisType;
    return cTypeReference(tg, args, t->nargs + 1);
  }
  if (t->flags & TF_Intersection) {
    bool changed = false;
    TsType **types = ts_arena_alloc(sizeof(TsType *) * (size_t)(t->ntypes ? t->ntypes : 1));
    for (int i = 0; i < t->ntypes; i++) {
      types[i] = cGetTypeWithThisArgument(t->types[i], thisArg);
      if (types[i] != t->types[i]) changed = true;
    }
    return changed ? cGetIntersectionType(types, t->ntypes) : t;
  }
  return t;
}

/* resolveObjectTypeMembers (62289): the members of type are the declared members of src, instantiated under
 * the mapper from typeParams to typeArgs (the identity when they are equal), then the inherited members of
 * the base types; the last type argument is the this argument of the bases. */
static void cResolveObjectMembers(TsType *t, TsType *src, TsType **typeParams, TsType **typeArgs, int n) {
  bool identity = true;
  for (int i = 0; identity && i < n; i++) identity = typeParams[i] == typeArgs[i];
  TsMapper *m = NULL;
  TsSymTable *own;
  if (identity) {
    own = cDeclaredMembers(src);
    if (!own) own = bTableNew();
  } else {
    m = cCreateTypeMapper(typeParams, typeArgs, n);
    /* createInstantiatedSymbolTable: with a single type parameter (this) the thisless members are shared */
    own = cInstantiateMembers(cDeclaredMembers(src), m, n == 1);
  }
  cResolveBaseTypes(src);
  TsSymTable *res = src->nbases ? cTableCopy(own) : own;
  TsType *thisArg = n ? typeArgs[n - 1] : NULL;
  t->instBases = ts_arena_alloc(sizeof(TsType *) * (size_t)(src->nbases ? src->nbases : 1));
  t->ninstBases = 0;
  for (int i = 0; i < src->nbases; i++) {
    TsType *bi = m ? cInstantiate(src->bases[i], m) : src->bases[i];
    if (thisArg) bi = cGetTypeWithThisArgument(bi, thisArg);
    if (bi == cErrorType) continue;
    t->instBases[t->ninstBases++] = bi;
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

/* the key of a string literal type: its value without the quotes */
static BName cLiteralKey(TsType *l) {
  if (l->textLen >= 2 && (l->text[0] == '"' || l->text[0] == '\'')) return bnEscapeText(l->text + 1, l->textLen - 2);
  return bnEscapeText(l->text, l->textLen);
}

/* the modifiers type of a mapped type over keyof T, or over a type parameter whose constraint is keyof T
 * (getModifiersTypeFromMappedType): the optional flags of its properties are copied */
static TsType *cMappedModifiersType(TsNode *tp, TsMapper *m) {
  TsNode *c = tp->constraint;
  if (!c) return NULL;
  if (c->kind == TSK_TypeOperator && c->extra == TSK_KeyOfKeyword) return cTypeFromNodeM(c->type, m);
  if (c->kind == TSK_TypeReference && c->typeName && c->typeName->kind == TSK_Identifier) {
    TsSymbol *s = cSymbolOfTypeName(c, c->typeName);
    if (s && (s->flags & SF_TypeParameter) && s->ndecls) {
      TsNode *pd = s->decls[0];
      if (pd->constraint && pd->constraint->kind == TSK_TypeOperator && pd->constraint->extra == TSK_KeyOfKeyword)
        return cTypeFromNodeM(pd->constraint->type, m);
    }
  }
  return NULL;
}

/* the type of a mapped member: the template with the key bound to the type parameter (T[P] is evaluated
 * as the property type of T under the key, the other templates as type nodes under the mapper) */
static TsType *cMappedTemplate(TsNode *tpl, TsMapper *mm) {
  if (tpl->kind != TSK_IndexedAccessType) return cTypeFromNodeM(tpl, mm);
  TsType *o = cTypeFromNodeM(tpl->objectType, mm);
  TsType *ix = cTypeFromNodeM(tpl->indexType, mm);
  if (o == cErrorType || ix == cErrorType) return cErrorType;
  if (!(ix->flags & TF_StringLiteral) || !(o->flags & TF_Object)) return cUnported("indexed access with a non-literal index");
  TsSymbol *p = cPropertyOf(o, cLiteralKey(ix));
  if (!p) return cUnported("indexed access: no such property");
  return cTypeOfSymbol(p);
}

/* resolveMappedTypeMembers (for constraints that are string literals or unions of them) */
static void cResolveMappedMembers(TsType *t) {
  TsNode *mn = t->typeNode;
  TsNode *tp = mn->typeParameter;
  TsType *P = cTypeOfTypeParameter(cGetMergedSymbol(tp->symbol));
  t->members = bTableNew();
  TsType *c = cInstantiate(t->mapConstraint, t->mapper);
  if (c == cErrorType) return;
  TsType **keys = c->flags & TF_Union ? c->types : &c;
  int nk = c->flags & TF_Union ? c->ntypes : 1;
  TsType *mod = mn->questionToken ? NULL : cMappedModifiersType(tp, t->mapper);
  /* the index signatures of the string and number keys (addMemberForKeyType): the template with the key bound to the key type */
  CIndexInfo *ix = ts_arena_alloc(sizeof(CIndexInfo) * (size_t)(nk ? nk : 1));
  int nix = 0;
  for (int i = 0; i < nk; i++) {
    TsType *k = keys[i];
    if (k->flags & (TF_String | TF_Number)) {
      if (mn->readonlyToken) { cUnported("readonly mapped type over an index key"); return; }
      TsMapper *mi = cCombineMappers(t->mapper, cCreateTypeMapper(&P, &k, 1));
      CIndexInfo ii;
      ii.keyType = k;
      ii.type = cMappedTemplate(mn->type, mi);
      ii.isReadonly = false;
      ii.decl = NULL;
      if (ii.type == cErrorType) return;
      ix[nix++] = ii;
      continue;
    }
    /* a unique symbol key is the property named by its symbol (isTypeUsableAsPropertyName), as an element access reads it */
    if (!(k->flags & (TF_StringLiteral | TF_UniqueESSymbol))) { cUnported("mapped type over a non-literal key"); return; }
    BName key = (k->flags & TF_UniqueESSymbol) ? (BName){ k->text, k->textLen } : cLiteralKey(k);
    TsSymbol *sym = cCreateSymbol(SF_Property, key);
    if (mn->questionToken) sym->flags |= SF_Optional;
    else if (mod && (mod->flags & TF_Object)) {
      TsSymbol *mp = cPropertyOf(mod, key);
      if (mp && (mp->flags & SF_Optional)) sym->flags |= SF_Optional;
    }
    TsMapper *mm = cCombineMappers(t->mapper, cCreateTypeMapper(&P, &k, 1));
    sym->type = cMappedTemplate(mn->type, mm);
    bTableSet(t->members, key, sym);
  }
  if (nix) { t->index = ix; t->nindex = nix; }
}

/* resolved members of an object type (resolveStructuredTypeMembers) */
static void cResolveMembers(TsType *t) {
  if (t->resolved) return;
  t->resolved = true;
  if (t->objectFlags & OF_Mapped) { cResolveMappedMembers(t); return; }
  if (t->objectFlags & OF_Reference) {
    /* resolveTypeReferenceMembers (62338): the type parameters and the this type of the target, and the type
     * arguments padded with the type itself as the this argument */
    TsType *tg = t->target;
    int ntp = tg->ntypeParams + (tg->thisType ? 1 : 0);
    TsType **tps = ts_arena_alloc(sizeof(TsType *) * (size_t)(ntp ? ntp : 1));
    if (tg->ntypeParams) memcpy(tps, tg->typeParams, sizeof(TsType *) * (size_t)tg->ntypeParams);
    if (tg->thisType) tps[ntp - 1] = tg->thisType;
    TsType **args = t->args;
    int nargs = t->nargs;
    if (nargs != ntp) {
      args = ts_arena_alloc(sizeof(TsType *) * (size_t)(nargs + 1));
      if (nargs) memcpy(args, t->args, sizeof(TsType *) * (size_t)nargs);
      args[nargs] = t;
      nargs++;
    }
    cResolveObjectMembers(t, tg, tps, args, ntp);
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
    cResolveObjectMembers(t, t, NULL, NULL, 0);
    return;
  }
  /* resolveAnonymousTypeMembers: the exports of a class, an enum, a value module (and a function merged with one) */
  if (t->symbol && (t->symbol->flags & (SF_Class | SF_Enum | SF_ValueModule | SF_Function | SF_Method))) {
    t->members = t->symbol->exports ? t->symbol->exports : t->symbol->members;
    return;
  }
  t->members = t->symbol ? t->symbol->members : NULL;
}

/* getPropertiesOfObjectType: the property symbols in member order (signatures excluded) */
static int cPropertiesOf(TsType *t, TsSymbol ***out) {
  if (t->flags & (TF_Union | TF_Intersection)) return cPropertiesOfUnionOrIntersection(t, out);
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
  if (t->flags & (TF_Union | TF_Intersection)) return cGetPropertyOfUnionOrIntersection(t, name);
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
    t->call = ts_arena_zalloc(sizeof(CSig) * (size_t)(t->ncall ? t->ncall : 1));
    for (int i = 0; i < t->ncall; i++) { t->call[i] = t->target->call[i]; t->call[i].mapper = t->mapper; cSigInstantiate(&t->call[i], &t->target->call[i], t->mapper); }
    t->construct = ts_arena_zalloc(sizeof(CSig) * (size_t)(t->nconstruct ? t->nconstruct : 1));
    for (int i = 0; i < t->nconstruct; i++) { t->construct[i] = t->target->construct[i]; t->construct[i].mapper = t->mapper; cSigInstantiate(&t->construct[i], &t->target->construct[i], t->mapper); }
    return;
  }
  TsSymbol *callSym = NULL, *newSym = NULL;
  if (t->typeNode && (t->typeNode->kind == TSK_FunctionType || t->typeNode->kind == TSK_ConstructorType)) {
    bool ctor = t->typeNode->kind == TSK_ConstructorType;
    t->call = ts_arena_zalloc(sizeof(CSig));
    t->call[0].decl = t->typeNode;
    t->call[0].isConstruct = ctor;
    t->call[0].mapper = t->sigMapper;
    cSigInstantiate(&t->call[0], &t->call[0], t->sigMapper);
    t->ncall = ctor ? 0 : 1;
    t->construct = ctor ? t->call : NULL;
    t->nconstruct = ctor ? 1 : 0;
    t->call = ctor ? NULL : t->call;
    return;
  }
  if (t->objectFlags & OF_Anonymous && t->symbol && (t->symbol->flags & (SF_Method | SF_Function))) {
    TsSymbol *s = t->symbol;
    t->call = ts_arena_zalloc(sizeof(CSig) * (size_t)(s->ndecls ? s->ndecls : 1));
    t->ncall = 0;
    for (int d = 0; d < s->ndecls; d++) {
      /* getSignaturesOfSymbol: the implementation of an overloaded function (a body right after a declaration of the same kind
       * and parent) is not a signature */
      if (d > 0 && s->decls[d]->body && s->decls[d]->parent == s->decls[d - 1]->parent &&
          s->decls[d]->kind == s->decls[d - 1]->kind && s->decls[d]->pos == s->decls[d - 1]->end)
        continue;
      t->call[t->ncall].decl = s->decls[d];
      t->call[t->ncall].isConstruct = false;
      t->call[t->ncall].mapper = t->sigMapper;
      cSigInstantiate(&t->call[t->ncall], &t->call[t->ncall], t->sigMapper);
      t->ncall++;
    }
    return;
  }
  if (t->members) {
    callSym = bTableGet(t->members, bnC("__call"));
    newSym = bTableGet(t->members, bnC("__new"));
  }
  if (callSym) {
    t->call = ts_arena_zalloc(sizeof(CSig) * (size_t)callSym->ndecls);
    for (int d = 0; d < callSym->ndecls; d++) {
      t->call[t->ncall].decl = callSym->decls[d];
      t->call[t->ncall].isConstruct = false;
      t->call[t->ncall].mapper = t->sigMapper;
      cSigInstantiate(&t->call[t->ncall], &t->call[t->ncall], t->sigMapper);
      t->ncall++;
    }
  }
  if (newSym) {
    t->construct = ts_arena_zalloc(sizeof(CSig) * (size_t)newSym->ndecls);
    for (int d = 0; d < newSym->ndecls; d++) {
      t->construct[t->nconstruct].decl = newSym->decls[d];
      t->construct[t->nconstruct].isConstruct = true;
      t->construct[t->nconstruct].mapper = t->sigMapper;
      cSigInstantiate(&t->construct[t->nconstruct], &t->construct[t->nconstruct], t->sigMapper);
      t->nconstruct++;
    }
  }
}

/* index infos: the __index declarations with one parameter, instantiated with the type's mapper */
static void cResolveIndexInfos(TsType *t) {
  if (t->indexResolved) return;
  cResolveMembers(t);
  t->indexResolved = true;
  if (!t->members) return;
  TsSymbol *idx = bTableGet(t->members, bnC("__index"));
  int nown = idx ? idx->ndecls : 0;
  /* own index signatures first, then the inherited ones whose key type the type does not declare (resolveObjectTypeMembers) */
  int cap = nown;
  for (int b = 0; b < t->ninstBases; b++) {
    cResolveIndexInfos(t->instBases[b]);
    cap += t->instBases[b]->nindex;
  }
  if (cap == 0) return;
  t->index = ts_arena_alloc(sizeof(CIndexInfo) * (size_t)cap);
  for (int d = 0; d < nown; d++) {
    TsNode *decl = idx->decls[d];
    if (decl->parameters->n != 1) { cUnported("index signature arity"); continue; }
    CIndexInfo ii;
    ii.keyType = cTypeFromNodeM(decl->parameters->v[0]->type, t->sigMapper);
    ii.type = cTypeFromNodeM(decl->type, t->sigMapper);
    ii.isReadonly = (bModifierFlags(decl) & TMF_Readonly) != 0;
    ii.decl = decl;
    t->index[t->nindex++] = ii;
  }
  for (int b = 0; b < t->ninstBases; b++) {
    TsType *bt = t->instBases[b];
    for (int k = 0; k < bt->nindex; k++) {
      bool own = false;
      for (int q = 0; q < t->nindex && !own; q++) own = t->index[q].keyType == bt->index[k].keyType;
      if (!own) t->index[t->nindex++] = bt->index[k];
    }
  }
}

/* ---- types of symbols (getTypeOfSymbol) ---- */
/* the type of an unannotated variable, property or parameter from its initializer (getWidenedTypeForVariableLikeDeclaration:
 * a mutable location widens its fresh literal types). The symbol is marked while its initializer is checked, so a circular
 * reference is unported. */
static TsType *cInitializedSymbolType(TsSymbol *sym, TsNode *d, bool member) {
  sym->type = cErrorType;
  TsNode *init = d->initializer ? d->initializer : d->name;
  TsType *t = cExprType(init);
  if (t == cErrorType) return t;
  bool readonly = (bModifierFlags(d) & TMF_Readonly) != 0;
  if (member && readonly) return t;
  if (!member && cIsConstVariableDecl(d)) return t;
  return cMutableLocationType(init, t);
}

/* the declared type of a parameter (getTypeOfParameter): an annotation, with undefined when it is optional */
static TsType *cUnannotatedParamType(TsSymbol *sym, TsNode *d);   /* ts-checker-body.h: contextual parameter types */
static bool cIsUntypedDeclParam(TsNode *p);                       /* ts-checker-body.h */
static bool cIsInitializedDeclParam(TsNode *p);                   /* ts-checker-body.h */
static TsType *cBindingElementType(TsSymbol *sym, TsNode *d);     /* ts-checker-body.h: destructured variables */
static TsType *cParameterSymbolType(TsSymbol *sym, TsNode *d) {
  if (!d->type) return cUnannotatedParamType(sym, d);
  /* an annotation is the declared type whatever the name (tryGetTypeFromEffectiveTypeNode), a binding pattern included */
  TsType *t = cTypeFromNode(d->type);
  if (d->questionToken || (d->initializer && t != cErrorType)) t = cAddUndefined(t);
  return t;
}

/* ---- union and intersection properties (createUnionOrIntersectionProperty 63588, getUnionOrIntersectionProperty
 * 63737, getPropertiesOfUnionOrIntersectionType 63213). A property of a union or intersection is the single
 * member property when every member has the same one (no symbol); otherwise a synthetic symbol, cached per type
 * (propertyCache), whose type is the union or intersection of the member property types (deferred to the first
 * getTypeOfSymbol when there are more than two). ---- */
enum { CK_Instantiated = 1, CK_SyntheticProperty = 2, CK_SyntheticMethod = 4, CK_Readonly = 8, CK_ReadPartial = 16,
       CK_WritePartial = 32, CK_HasNonUniformType = 64, CK_HasLiteralType = 128, CK_ContainsPublic = 256,
       CK_ContainsProtected = 512, CK_ContainsPrivate = 1024, CK_ContainsStatic = 2048, CK_DeferredType = 65536,
       CK_HasNeverType = 131072 };
#define CK_Partial (CK_ReadPartial | CK_WritePartial)
#define SF_ClassMemberFlags 106500u   /* ClassMember: Method | Accessor | Property */

static TsType *cApparentType(TsType *s);   /* ts-relate.h: the apparent type of a primitive (NULL for other types) */

/* getDeclarationModifierFlagsFromSymbol (accessibility and readonly bits; a synthetic symbol has its own) */
static uint32_t cSymModifierFlags(TsSymbol *s) {
  if (s->valueDeclaration) return bModifierFlags(s->valueDeclaration);
  if (s->checkFlags & (CK_SyntheticProperty | CK_SyntheticMethod)) {
    uint32_t access = (s->checkFlags & CK_ContainsPrivate) ? TMF_Private : (s->checkFlags & CK_ContainsPublic) ? TMF_Public : TMF_Protected;
    return access;
  }
  return 0;
}

/* getTargetSymbol: the symbol an instantiated symbol instantiates (a symbol itself otherwise) */
static TsSymbol *cSymTargetOrSelf(TsSymbol *s) { return s->instTarget ? s->instTarget : s; }

/* isReadonlySymbol (the checks the union properties need) */
static bool cSymIsReadonly(TsSymbol *s) {
  if (s->checkFlags & CK_Readonly) return true;
  if ((s->flags & SF_Property) && (cSymModifierFlags(s) & TMF_Readonly)) return true;
  if ((s->flags & SF_Accessor) && !(s->flags & SF_SetAccessor)) return true;
  for (int i = 0; i < s->ndecls; i++)
    if (cIsConstVariableDecl(s->decls[i]) && (s->flags & SF_Variable)) return true;
  return false;
}

/* isPrototypeProperty: a method, or a synthetic method */
static bool cIsPrototypeProperty(TsSymbol *s) { return (s->flags & SF_Method) || (s->checkFlags & CK_SyntheticMethod); }

/* compareProperties2 with the identity comparison of the types (the isInstantiation test of the union property) */
static bool cSameProperty(TsSymbol *a, TsSymbol *b) {
  if (a == b) return true;
  uint32_t accA = cSymModifierFlags(a) & (TMF_Private | TMF_Protected), accB = cSymModifierFlags(b) & (TMF_Private | TMF_Protected);
  if (accA != accB) return false;
  if (accA) {
    if (cSymTargetOrSelf(a) != cSymTargetOrSelf(b)) return false;
  } else if ((a->flags & SF_Optional) != (b->flags & SF_Optional)) {
    return false;
  }
  if (cSymIsReadonly(a) != cSymIsReadonly(b)) return false;
  return cTypeOfSymbol(a) == cTypeOfSymbol(b);
}

/* getLocalTypeParametersOfClassOrInterfaceOrTypeAlias is not empty: a declaration of the symbol has type parameters */
static bool cHasLocalTypeParams(TsSymbol *s) {
  for (int i = 0; i < s->ndecls; i++) {
    TsNode *d = s->decls[i];
    if ((d->kind == TSK_ClassDeclaration || d->kind == TSK_ClassExpression || d->kind == TSK_InterfaceDeclaration ||
         d->kind == TSK_TypeAliasDeclaration) && d->typeParameters && d->typeParameters->n)
      return true;
  }
  return false;
}

/* the members of a union or intersection, as getPropertyOfType and getPropertiesOfType see them */
static TsSymbol *cPropertyOfMember(TsType *t, BName name);
static int cPropertiesOfMember(TsType *t, TsSymbol ***out);
static TsSymbol *cGetPropertyOfUnionOrIntersection(TsType *t, BName name);

/* the applicable index info of a property name (getApplicableIndexInfoForName, the string index): NULL when none;
 * not ported when the name is numeric-like, where the number index may apply too */
static CIndexInfo *cIndexInfoForName(TsType *o, BName name, bool *unp) {
  cResolveIndexInfos(o);
  *unp = false;
  CIndexInfo *hit = NULL;
  for (int i = 0; i < o->nindex; i++) {
    CIndexInfo *ii = &o->index[i];
    if (ii->keyType == cStringType) { if (!hit) hit = ii; }
    else if (ii->keyType == cNumberType && name.n > 0 && name.s[0] >= '0' && name.s[0] <= '9') *unp = true;
  }
  return hit;
}

/* createUnionOrIntersectionProperty (node 63588): a new symbol unless the members agree on one property symbol */
static TsSymbol *cCreateUnionOrIntersectionProperty(TsType *ct, BName name) {
  bool isUnion = (ct->flags & TF_Union) != 0;
  uint32_t propFlags = 0, syntheticFlag = CK_SyntheticMethod, checkFlags = isUnion ? 0 : CK_Readonly;
  uint32_t optionalFlag = 0;
  bool haveOptional = false, mergedInstantiations = false;
  TsSymbol *singleProp = NULL;
  TsSymbol **propSet = NULL;
  int nprop = 0;
  TsType **indexTypes = NULL;
  int nindex = 0;
  for (int i = 0; i < ct->ntypes; i++) {
    TsType *cur = ct->types[i];
    TsType *type = (cur->flags & TF_Object) || (cur->flags & (TF_Union | TF_Intersection)) ? cur : cApparentType(cur);
    if (!type) { (void)cUnported("union property of a member without apparent type"); return NULL; }
    if (type == cErrorType || (type->flags & TF_Never)) continue;
    TsSymbol *prop = cPropertyOfMember(type, name);
    uint32_t modifiers = prop ? cSymModifierFlags(prop) : 0;
    if (prop) {
      if (prop->flags & SF_ClassMemberFlags) {
        if (!haveOptional) { haveOptional = true; optionalFlag = isUnion ? 0 : SF_Optional; }
        if (isUnion) optionalFlag |= prop->flags & SF_Optional;
        else optionalFlag &= prop->flags;
      }
      if (!singleProp) {
        singleProp = prop;
        propFlags = (prop->flags & SF_Accessor) ? (prop->flags & SF_Accessor) : SF_Property;
      } else if (prop != singleProp) {
        bool isInstantiation = cSymTargetOrSelf(prop) == cSymTargetOrSelf(singleProp);
        if (isInstantiation && cSameProperty(singleProp, prop)) {
          mergedInstantiations = singleProp->parent && cHasLocalTypeParams(singleProp->parent);
        } else {
          if (!propSet) { propSet = ts_arena_alloc(sizeof(TsSymbol *) * (size_t)(ct->ntypes + 1)); propSet[nprop++] = singleProp; }
          bool seen = false;
          for (int k = 0; k < nprop && !seen; k++) seen = propSet[k] == prop;
          if (!seen) propSet[nprop++] = prop;
        }
        if ((propFlags & SF_Accessor) && (prop->flags & SF_Accessor) != (propFlags & SF_Accessor))
          propFlags = (propFlags & ~SF_Accessor) | SF_Property;
      }
      if (isUnion && cSymIsReadonly(prop)) checkFlags |= CK_Readonly;
      else if (!isUnion && !cSymIsReadonly(prop)) checkFlags &= ~CK_Readonly;
      checkFlags |= ((modifiers & (TMF_Private | TMF_Protected)) ? 0 : CK_ContainsPublic) |
                    ((modifiers & TMF_Protected) ? CK_ContainsProtected : 0) | ((modifiers & TMF_Private) ? CK_ContainsPrivate : 0);
      if (!cIsPrototypeProperty(prop)) syntheticFlag = CK_SyntheticProperty;
    } else if (isUnion) {
      if (type->flags & TF_Object) {
        bool unp;
        BName nm = name;
        CIndexInfo *ii = cIndexInfoForName(type, nm, &unp);
        if (unp) { (void)cUnported("union property with a numeric name"); return NULL; }
        if (ii) {
          propFlags = (propFlags & ~SF_Accessor) | SF_Property;
          checkFlags |= CK_WritePartial | (ii->isReadonly ? CK_Readonly : 0);
          if (type->objectFlags & OF_Tuple) { (void)cUnported("union property of a tuple index"); return NULL; }
          if (!indexTypes) indexTypes = ts_arena_alloc(sizeof(TsType *) * (size_t)(ct->ntypes + 1));
          indexTypes[nindex++] = ii->type;
        } else if (type->objectFlags & OF_ObjectLiteral) {
          checkFlags |= CK_WritePartial;
          if (!indexTypes) indexTypes = ts_arena_alloc(sizeof(TsType *) * (size_t)(ct->ntypes + 1));
          indexTypes[nindex++] = cUndefinedType;
        } else {
          checkFlags |= CK_ReadPartial;
        }
      } else {
        checkFlags |= CK_ReadPartial;
      }
    }
  }
  if (!singleProp) return NULL;
  /* the union of partial properties with private or protected members needs their common declarations */
  if (isUnion && (propSet || (checkFlags & CK_Partial)) && (checkFlags & (CK_ContainsPrivate | CK_ContainsProtected))) {
    bool commonOk = true;
    if (propSet) for (int k = 0; k < nprop; k++) if (propSet[k]->ndecls == 0 && !propSet[k]->valueDeclaration) commonOk = false;
    if (!(propSet && commonOk)) return NULL;
  }
  if (!propSet && !(checkFlags & CK_ReadPartial) && !nindex) {
    if (!mergedInstantiations) return singleProp;
    /* createSymbolWithType (the clone of the single declaration, its parent the declaring symbol of the declaration) */
    TsNode *vd = singleProp->valueDeclaration;
    TsSymbol *clone = cCreateSymbol(singleProp->flags, name);
    clone->checkFlags = singleProp->checkFlags & CK_Readonly;
    clone->decls = singleProp->decls;
    clone->ndecls = singleProp->ndecls;
    clone->declCap = singleProp->ndecls;
    clone->valueDeclaration = vd;
    clone->parent = vd && vd->symbol ? vd->symbol->parent : NULL;
    clone->type = singleProp->type;
    return clone;
  }
  TsSymbol **props = propSet ? propSet : &singleProp;
  int np = propSet ? nprop : 1;
  TsNode *firstVD = NULL;
  bool nonUniformVD = false;
  TsType **propTypes = ts_arena_alloc(sizeof(TsType *) * (size_t)(np + nindex + 1));
  int nt = 0;
  TsType *firstType = NULL;
  for (int k = 0; k < np; k++) {
    TsSymbol *prop = props[k];
    if (prop->flags & SF_Accessor) { (void)cUnported("accessor in a union property"); return NULL; }
    if (!firstVD) firstVD = prop->valueDeclaration;
    else if (prop->valueDeclaration && prop->valueDeclaration != firstVD) nonUniformVD = true;
    TsType *type = cTypeOfSymbol(prop);
    if (!firstType) firstType = type;
    if (type != firstType) checkFlags |= CK_HasNonUniformType;
    if (type->flags & (TF_BooleanLiteral | TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral | TF_UniqueESSymbol)) checkFlags |= CK_HasLiteralType;
    if (type->flags & TF_Never) checkFlags |= CK_HasNeverType;
    propTypes[nt++] = type;
  }
  for (int k = 0; k < nindex; k++) propTypes[nt++] = indexTypes[k];
  TsSymbol *result = cCreateSymbol(propFlags | (haveOptional ? optionalFlag : 0), name);
  result->checkFlags = syntheticFlag | checkFlags;
  if (!nonUniformVD && firstVD) {
    result->valueDeclaration = firstVD;
    if (firstVD->symbol && firstVD->symbol->parent) result->parent = firstVD->symbol->parent;
  }
  /* the declarations of the members */
  int nd = 0;
  for (int k = 0; k < np; k++) nd += props[k]->ndecls;
  result->decls = ts_arena_alloc(sizeof(TsNode *) * (size_t)(nd ? nd : 1));
  for (int k = 0; k < np; k++)
    for (int d = 0; d < props[k]->ndecls; d++) result->decls[result->ndecls++] = props[k]->decls[d];
  result->declCap = result->ndecls;
  if (nt > 2) {
    /* a deferred type (DeferredType): the union or intersection of the constituents at the first getTypeOfSymbol */
    result->checkFlags |= CK_DeferredType;
    result->deferParent = ct;
    result->deferTypes = propTypes;
    result->nDeferTypes = nt;
  } else {
    result->type = isUnion ? cGetUnionType(propTypes, nt) : cGetIntersectionType(propTypes, nt);
  }
  return result;
}

/* getUnionOrIntersectionProperty (63737): the property cached per union or intersection (propertyCache), NULL when
 * the members do not have it (the cache keeps the partial ones, which the caller filters) */
static TsSymbol *cGetUnionOrIntersectionProperty(TsType *t, BName name) {
  if (!t->unionProps) t->unionProps = bTableNew();
  TsSymbol *p = bTableGet(t->unionProps, name);
  if (p) return p;
  p = cCreateUnionOrIntersectionProperty(t, name);
  if (p) bTableSet(t->unionProps, name, p);
  return p;
}

/* getPropertyOfUnionOrIntersectionType: a property every member has (ReadPartial ones are missing) */
static TsSymbol *cGetPropertyOfUnionOrIntersection(TsType *t, BName name) {
  TsSymbol *p = cGetUnionOrIntersectionProperty(t, name);
  return p && !(p->checkFlags & CK_ReadPartial) ? p : NULL;
}

/* getPropertiesOfUnionOrIntersectionType (63213): the properties of the members, in the order of the first member
 * that has them (cached as resolvedProperties); a union stops after a member without index signatures */
static int cPropertiesOfUnionOrIntersection(TsType *t, TsSymbol ***out) {
  if (!t->resolvedPropsDone) {
    TsSymTable *members = bTableNew();
    bool isUnion = (t->flags & TF_Union) != 0;
    for (int i = 0; i < t->ntypes; i++) {
      TsType *cur = t->types[i];
      TsSymbol **props;
      int np = cPropertiesOfMember(cur, &props);
      for (int k = 0; k < np; k++) {
        if (bTableGet(members, props[k]->name)) continue;
        TsSymbol *combined = cGetPropertyOfUnionOrIntersection(t, props[k]->name);
        if (combined) bTableSet(members, props[k]->name, combined);
      }
      if (isUnion) {
        TsType *ap = (cur->flags & TF_Object) || (cur->flags & (TF_Union | TF_Intersection)) ? cur : cApparentType(cur);
        if (ap && (ap->flags & TF_Object)) { cResolveIndexInfos(ap); if (ap->nindex == 0) break; }
      }
    }
    t->resolvedProps = ts_arena_alloc(sizeof(TsSymbol *) * (size_t)(members->n ? members->n : 1));
    t->nResolvedProps = 0;
    for (int i = 0; i < members->n; i++) t->resolvedProps[t->nResolvedProps++] = members->e[i].sym;
    t->resolvedPropsDone = true;
  }
  *out = t->resolvedProps;
  return t->nResolvedProps;
}

/* the property symbols of a member of a union or intersection (getPropertiesOfType of the member) */
static int cPropertiesOfMember(TsType *t, TsSymbol ***out) {
  if (t->flags & (TF_Union | TF_Intersection)) return cPropertiesOfUnionOrIntersection(t, out);
  if (t->flags & TF_Object) return cPropertiesOf(t, out);
  TsType *a = cApparentType(t);
  if (!a) { *out = NULL; return 0; }
  return cPropertiesOf(a, out);
}

/* getPropertyOfType of a member of a union or intersection */
static TsSymbol *cPropertyOfMember(TsType *t, BName name) {
  if (t->flags & (TF_Union | TF_Intersection)) return cGetPropertyOfUnionOrIntersection(t, name);
  if (t->flags & TF_Object) return cPropertyOf(t, name);
  TsType *a = cApparentType(t);
  return a ? cPropertyOf(a, name) : NULL;
}

static TsType *cTypeOfSymbol(TsSymbol *sym) {
  if (sym == cUndefinedSymbol) return cUndefinedType; /* undefinedSymbol: undefinedWideningType */
  if (sym->instTarget) {   /* getTypeOfInstantiatedSymbol (61381) */
    if (!sym->type) sym->type = cInstantiate(cTypeOfSymbol(sym->instTarget), sym->instMapper);
    return sym->type;
  }
  if (sym->checkFlags & CK_DeferredType) {   /* getTypeOfSymbolWithDeferredType: the union or intersection of the member types */
    if (!sym->type) sym->type = sym->deferParent->flags & TF_Union ? cGetUnionType(sym->deferTypes, sym->nDeferTypes) : cGetIntersectionType(sym->deferTypes, sym->nDeferTypes);
    return sym->type;
  }
  sym = cGetMergedSymbol(sym);
  if (sym->type) return sym->type;
  if (!sym->valueDeclaration && !sym->ndecls) return cUnported("symbol without declarations");   /* intrinsic symbols such as arguments */
  TsType *t = NULL;
  if (sym->flags & SF_Alias) {
    TsSymbol *target = cResolveImportAlias(sym, 0);
    t = target ? cTypeOfSymbol(target) : cErrorType;
  } else if (sym->flags & SF_Variable) {
    TsNode *d = sym->valueDeclaration ? sym->valueDeclaration : sym->decls[0];
    if (d->kind == TSK_Parameter) t = cParameterSymbolType(sym, d);
    else if (d->kind == TSK_BindingElement) { sym->type = cErrorType; t = cBindingElementType(sym, d); }
    else if (d->kind != TSK_VariableDeclaration) t = cUnported("variable declaration kind");
    else if (d->type) t = cTypeFromNode(d->type);
    else if (cIsConstVariableDecl(d) && d->initializer && cIsSymbolCall(d->initializer)) t = cUniqueSymbolType(d);
    else if (d->initializer && d->name && d->name->kind == TSK_Identifier) t = cInitializedSymbolType(sym, d, false);
    else t = cUnported("variable without annotation");
  } else if (sym->flags & SF_Property) {
    TsNode *d = sym->valueDeclaration ? sym->valueDeclaration : sym->decls[0];
    if (d->kind == TSK_PropertyAssignment && d->initializer) t = cInitializedSymbolType(sym, d, true);
    else if (d->kind == TSK_ShorthandPropertyAssignment && d->name) t = cInitializedSymbolType(sym, d, true);
    else if (d->kind == TSK_PropertyDeclaration && !d->type && d->initializer && d->name && d->name->kind == TSK_Identifier)
      t = cInitializedSymbolType(sym, d, true);
    else if ((d->kind != TSK_PropertySignature && d->kind != TSK_PropertyDeclaration) || !d->type) t = cUnported("property without annotation");
    else {
      t = cTypeFromNode(d->type);
      /* addOptionality: T | undefined under strictNullChecks */
      if (d->questionToken) t = cAddOptionality(t);
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
  } else if (sym->flags & SF_EnumMember) {
    /* getTypeOfEnumMember: the enum literal type of the member */
    t = cEnumMemberType(sym);
  } else if (sym->flags & (SF_Enum | SF_ValueModule)) {
    /* getTypeOfFuncClassEnumModule: the anonymous object type of an enum or a value module, whose members are its exports */
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

/* the identity cache is a hash table of slots (cIdSlots holds index + 1 into cIdCache) */
static int32_t *cIdSlots;
static int cIdSlotCap;

static uint32_t cIdHash(TsType *s, TsType *t) {
  uint32_t h = (uint32_t)s->id * 2654435761u ^ (uint32_t)t->id * 40503u;
  return h ^ (h >> 15);
}

static void cIdRehash(int cap) {
  free(cIdSlots);
  cIdSlots = calloc((size_t)cap, sizeof(int32_t));
  if (!cIdSlots) abort();
  cIdSlotCap = cap;
  for (int i = 0; i < cIdN; i++) {
    uint32_t mask = (uint32_t)cap - 1, h = cIdHash(cIdCache[i].s, cIdCache[i].t) & mask;
    while (cIdSlots[h]) h = (h + 1) & mask;
    cIdSlots[h] = i + 1;
  }
}

static int cIdFind(TsType *s, TsType *t) {
  if (!cIdSlotCap) return -1;
  uint32_t mask = (uint32_t)cIdSlotCap - 1, h = cIdHash(s, t) & mask;
  for (;;) {
    int32_t v = cIdSlots[h];
    if (v == 0) return -1;
    if (cIdCache[v - 1].s == s && cIdCache[v - 1].t == t) return v - 1;
    h = (h + 1) & mask;
  }
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

/* getIndexedAccessType of a rest type at an index, distributed over a union (the union property of the members) */
static TsType *cIndexedAtPosition(TsType *rt, int index) {
  if (!(rt->flags & TF_Union)) return cGetIndexedAccessType(rt, cIndexLiteral(index), false);
  TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(rt->ntypes ? rt->ntypes : 1));
  for (int i = 0; i < rt->ntypes; i++) {
    parts[i] = cIndexedAtPosition(rt->types[i], index);
    if (parts[i] == cErrorType) return parts[i];
  }
  return cGetUnionType(parts, rt->ntypes);
}

/* the element of a rest parameter's type at an index (getIndexedAccessType of a tuple past its length is absent) */
static TsType *cRestElementAt(TsType *rt, int index, bool *ok) {
  if (rt == cErrorType) return rt;
  if ((rt->objectFlags & OF_Tuple) && index >= rt->nargs) { *ok = false; return NULL; }
  return cIndexedAtPosition(rt, index);
}
/* the type of a parameter at a position (getTypeAtPosition): a rest parameter gives its element type */
static TsType *cParamTypeAt(CParams *cp, int pos, bool *ok, TsMapper *m) {
  TsNode *p = pos < cp->n ? cp->params[pos] : cp->rest;
  *ok = true;
  if (!p) { *ok = false; return cErrorType; }
  if (p->dotDotDotToken) {
    /* getTypeAtPosition: the rest type indexed by the position past the other parameters (getIndexedAccessType); a tuple
     * rest has no type beyond its length */
    if (!p->type) return cUnported("rest parameter element type");
    TsType *rt = cTypeFromNodeM(p->type, m);
    return cRestElementAt(rt, pos - (cp->n - 1), ok);   /* the rest parameter is the last of cp->params */
  }
  bool objMethod = !p->type && p->parent && p->parent->kind == TSK_MethodDeclaration && p->parent->parent &&
                   p->parent->parent->kind == TSK_ObjectLiteralExpression;
  if (!p->type && p->parent && (p->parent->kind == TSK_FunctionExpression || p->parent->kind == TSK_ArrowFunction || objMethod)) {
    /* an unannotated parameter of a function expression or arrow: the type of its symbol (the contextual type) */
    if (!p->symbol) return cUnported("parameter without annotation");
    TsType *ct = cTypeOfSymbol(p->symbol);
    return m && ct != cErrorType ? cInstantiate(ct, m) : ct;
  }
  if (!p->type && p->symbol && cIsInitializedDeclParam(p)) {
    /* a parameter of a function declaration or method with an initializer: its symbol type (getTypeOfParameter) */
    TsType *ct = cTypeOfSymbol(p->symbol);
    return m && ct != cErrorType ? cInstantiate(ct, m) : ct;
  }
  if (cIsUntypedDeclParam(p)) return cAnyType;
  TsType *t = cTypeFromNodeM(p->type, m);
  if (p->questionToken) t = cAddOptionality(t);
  return t;
}

/* instantiateSignature (67xxx): the parameters of a signature instance are instantiated symbols. Each parameter
 * symbol of the source signature (its declared symbol, or the instance of an earlier instantiation) is passed to
 * instantiateSymbol with the mapper, as node's instantiateList(signature.parameters, mapper, instantiateSymbol), and
 * the this parameter likewise. Their types are resolved on demand (getTypeOfInstantiatedSymbol). A NULL mapper
 * instantiates nothing; a declared symbol that is not generic is kept by instantiateSymbol. */
static void cSigInstantiate(CSig *dst, const CSig *src, struct TsMapper *m) {
  dst->psyms = NULL;
  dst->thisSym = NULL;
  dst->retCell = NULL;
  if (!m) return;
  dst->retCell = ts_arena_alloc(sizeof(TsType *));
  *dst->retCell = NULL;
  TsList *pl = dst->decl->parameters;
  int np = 0;
  for (int i = 0; pl && i < pl->n; i++) if (!cIsThisParam(pl->v[i])) np++;
  TsSymbol **ps = ts_arena_alloc(sizeof(TsSymbol *) * (size_t)(np ? np : 1));
  TsSymbol *ths = NULL;
  int k = 0;
  for (int i = 0; pl && i < pl->n; i++) {
    TsNode *p = pl->v[i];
    bool isThis = cIsThisParam(p);
    TsSymbol *s = isThis ? (src->thisSym ? src->thisSym : p->symbol) : (src->psyms ? src->psyms[k] : p->symbol);
    if (!s) cUnported("parameter without symbol");
    if (isThis) ths = s ? cInstantiateSymbol(s, m) : NULL;
    else ps[k++] = s ? cInstantiateSymbol(s, m) : NULL;
  }
  dst->psyms = ps;
  dst->thisSym = ths;
}

/* the type of a parameter of a signature instance: the parameter symbol of the instance (getTypeOfSymbol), so each
 * parameter is instantiated once per signature instance. The rest parameter and the positions past the list keep the
 * element type of the rest array (cParamTypeAt). */
static TsType *cSigParamType(CSig *sig, CParams *cp, int pos, bool *ok) {
  /* a rest parameter of an instance: its type is the instantiated symbol's (getTypeOfSymbol, cached on the symbol) */
  if (sig->mapper && sig->psyms && cp->rest && cp->n && cp->params[cp->n - 1] == cp->rest && pos >= cp->n - 1 &&
      sig->psyms[cp->n - 1] && cp->rest->type) {
    *ok = true;
    return cRestElementAt(cTypeOfSymbol(sig->psyms[cp->n - 1]), pos - (cp->n - 1), ok);
  }
  if (!sig->mapper || !sig->psyms || pos >= cp->n || !sig->psyms[pos] || cp->params[pos]->dotDotDotToken)
    return cParamTypeAt(cp, pos, ok, sig->mapper);
  *ok = true;
  return cTypeOfSymbol(sig->psyms[pos]);
}

/* the declared type parameters of a signature declaration (typeParameters) */
static TsType **cDeclTypeParamsOfNode(TsNode *d, int *n) {
  *n = d->typeParameters ? d->typeParameters->n : 0;
  TsType **tps = ts_arena_alloc(sizeof(TsType *) * (size_t)(*n ? *n : 1));
  for (int i = 0; i < *n; i++) tps[i] = cTypeOfTypeParameter(cGetMergedSymbol(d->typeParameters->v[i]->symbol));
  return tps;
}

static bool cCompareSignaturesIdentical(CSig *a, CSig *b) {
  if (a->decl == b->decl && a->mapper == b->mapper) return true;
  TsNode *da = a->decl, *db = b->decl;
  TsMapper *ma = a->mapper, *mb = b->mapper;
  CParams pa, pb;
  if (!cSplitParams(da, &pa) || !cSplitParams(db, &pb)) { cUnported("too many parameters"); return true; }
  /* isMatchingSignature: parameter count, min count and rest parameter */
  bool restA = pa.rest != NULL, restB = pb.rest != NULL;
  if (pa.n != pb.n || pa.min != pb.min || restA != restB) return false;
  /* the type parameters (compareSignaturesIdentical): the same number, the constraints and defaults identical under
   * the mapping from the source's to the target's, then the source is erased under that mapping */
  int na = da->typeParameters ? da->typeParameters->n : 0, nb = db->typeParameters ? db->typeParameters->n : 0;
  if (na != nb) return false;
  if (na) {
    int n;
    TsType **sTps = cDeclTypeParamsOfNode(da, &n), **tTps = cDeclTypeParamsOfNode(db, &n);
    TsMapper *tpm = cCreateTypeMapper(sTps, tTps, na);
    for (int i = 0; i < na; i++) {
      if (sTps[i] == tTps[i]) continue;
      TsType *cs = cConstraintOf(sTps[i]), *ct = cConstraintOf(tTps[i]);
      TsNode *dsN = cTypeParamDefaultNode(sTps[i]), *dtN = cTypeParamDefaultNode(tTps[i]);
      TsType *ds = dsN ? cTypeFromNode(dsN) : NULL, *dt = dtN ? cTypeFromNode(dtN) : NULL;
      if (ds == cErrorType || dt == cErrorType) { cUnported("type parameter default"); return true; }
      TsType *csI = cs ? cInstantiate(cs, tpm) : cUnknownType;
      if (!cIdentical(csI, ct ? ct : cUnknownType)) return false;
      TsType *dsI = ds ? cInstantiate(ds, tpm) : cUnknownType;
      if (!cIdentical(dsI, dt ? dt : cUnknownType)) return false;
    }
    ma = cCombineMappers(ma, tpm);   /* instantiateSignature(source, mapper, true) */
  }
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

static void cIdentityStore(TsType *s, TsType *t, bool r) {
  if (cIdN == cIdCap) {
    cIdCap = cIdCap ? cIdCap * 2 : 256;
    cIdCache = realloc(cIdCache, sizeof(CIdPair) * (size_t)cIdCap);
    if (!cIdCache) abort();
  }
  if ((cIdN + 1) * 2 > cIdSlotCap) cIdRehash(cIdSlotCap ? cIdSlotCap * 2 : 1024);
  cIdCache[cIdN].s = s;
  cIdCache[cIdN].t = t;
  cIdCache[cIdN].r = r;
  cIdN++;
  cIdentityCacheCount++;
  {
    uint32_t mask = (uint32_t)cIdSlotCap - 1, h = cIdHash(s, t) & mask;
    while (cIdSlots[h]) h = (h + 1) & mask;
    cIdSlots[h] = cIdN;
  }
}

/* isRelatedTo with the identity relation (checker.ts): the same flags, then the structure. Pairs that reach
 * recursiveTypeRelatedTo are cached under the key of the sorted ids (getRelationKey), so a pair and its reverse
 * share one entry; the identical singletons and the flag mismatches are not cached. */
static bool cIdentical(TsType *s, TsType *t) {
  s = cRegularType(s);
  t = cRegularType(t);
  if (s == t) return true;
  if (s == cErrorType || t == cErrorType) { cUnportedHit = true; return true; }
  if (s->flags != t->flags) return false;
  if (s->flags & (TF_Any | TF_Unknown | TF_String | TF_Number | TF_Boolean | TF_BigInt | TF_ESSymbol | TF_Void |
                  TF_Undefined | TF_Null | TF_Never | TF_NonPrimitive))
    return true;
  if (s->id > t->id) { TsType *x = s; s = t; t = x; }
  int ci = cIdFind(s, t);
  if (ci >= 0) return cIdCache[ci].r;
  bool r;
  if (s->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BooleanLiteral | TF_BigIntLiteral)) {
    r = false;   /* distinct literal objects */
  } else if (s->flags & (TF_Union | TF_Intersection)) {
    /* eachTypeRelatedToSomeType both ways */
    r = true;
    for (int i = 0; i < s->ntypes && r; i++) {
      bool hit = false;
      for (int j = 0; j < t->ntypes && !hit; j++) hit = cIdentical(s->types[i], t->types[j]);
      r = hit;
    }
    for (int j = 0; j < t->ntypes && r; j++) {
      bool hit = false;
      for (int i = 0; i < s->ntypes && !hit; i++) hit = cIdentical(t->types[j], s->types[i]);
      r = hit;
    }
  } else if (s->flags & TF_IndexedAccess) {
    return cIdentical(s->iaObj, t->iaObj) && cIdentical(s->iaIdx, t->iaIdx);
  } else if (s->flags & TF_Index) {
    return cIdentical(s->ixOf, t->ixOf);
  } else if (!(s->flags & TF_Object)) {
    return false;
  } else {
    /* the stack of pairs being compared (a pair on the stack is assumed related) */
    for (int i = 0; i < cIdDepth; i++)
      if (cIdentityStack[i].s == s && cIdentityStack[i].t == t) return true;
    if (cIdDepth >= 200) { cUnportedHit = true; return true; }
    cIdentityStack[cIdDepth].s = s;
    cIdentityStack[cIdDepth].t = t;
    cIdDepth++;
    if ((s->objectFlags & OF_Reference) && (t->objectFlags & OF_Reference) && s->target == t->target) {
      r = true;
      for (int i = 0; i < s->nargs && r; i++) r = cIdentical(s->args[i], t->args[i]);
    } else {
      /* other references (different targets) compare structurally */
      r = cIdentityStructural(s, t);
    }
    cIdDepth--;
  }
  cIdentityStore(s, t, r);
  return r;
}

/* Every checker run starts with empty caches: the arena that holds their entries is reset between runs. */
static void cResetTypeCaches(void) {
  cIdN = 0;
  if (cIdSlots) memset(cIdSlots, 0, sizeof(int32_t) * (size_t)cIdSlotCap);
  cLitN = 0;
  if (cLitSlots) memset(cLitSlots, 0, sizeof(int32_t) * (size_t)cLitSlotCap);
  cGlobalArrayType = NULL;
  cGlobalReadonlyArrayType = NULL;
  cNextSymbolId = 1;
  cUnportedCount = 0;
  cIdentityCacheCount = 0;
  cInstTotal = 0;
  cInstDepth = 0;
  cActiveN = 0;
  memset(cUnionTab, 0, sizeof cUnionTab);
  memset(cTupleTargets, 0, sizeof cTupleTargets);
  memset(cInterTab, 0, sizeof cInterTab);
  memset(cIndexedTab, 0, sizeof cIndexedTab);
  memset(cSigInstTab, 0, sizeof cSigInstTab);
}

/* isTypeIdenticalTo: the entry point used by the checks */
static bool cIsTypeIdenticalTo(TsType *s, TsType *t) { return cIdentical(s, t); }

#endif
