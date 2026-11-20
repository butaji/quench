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
enum { OF_Class = 1, OF_Interface = 2, OF_Reference = 4, OF_Anonymous = 16, OF_Instantiated = 64, OF_ObjectLiteral = 128 };

typedef struct CSig CSig;
typedef struct CIndexInfo { struct TsType *keyType, *type; bool isReadonly; } CIndexInfo;

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
} TsType;

struct CSig { TsNode *decl; bool isConstruct; TsType *thisType; };

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

/* the type of a type parameter symbol (createTypeParameter, cached on the symbol) */
static TsType *cTypeOfTypeParameter(TsSymbol *sym) {
  sym = cGetMergedSymbol(sym);
  if (sym->declaredType) return sym->declaredType;
  TsType *t = cNewType(TF_TypeParameter);
  t->symbol = sym;
  sym->declaredType = t;
  return t;
}

/* createTypeReference: a reference to a generic type with type arguments (cached per target and arguments) */
static TsType **cRefCache;
static int cRefN, cRefCap;
static TsType *cTypeReference(TsType *target, TsType **args, int nargs) {
  for (int i = 0; i < cRefN; i++) {
    TsType *r = cRefCache[i];
    if (r->target != target || r->nargs != nargs) continue;
    bool same = true;
    for (int k = 0; k < nargs && same; k++) same = r->args[k] == args[k];
    if (same) return r;
  }
  TsType *r = cNewType(TF_Object);
  r->objectFlags = OF_Reference;
  r->target = target;
  r->symbol = target->symbol;
  r->args = ts_arena_alloc(sizeof(TsType *) * (size_t)(nargs ? nargs : 1));
  memcpy(r->args, args, sizeof(TsType *) * (size_t)nargs);
  r->nargs = nargs;
  if (cRefN == cRefCap) {
    cRefCap = cRefCap ? cRefCap * 2 : 64;
    cRefCache = realloc(cRefCache, sizeof(TsType *) * (size_t)cRefCap);
    if (!cRefCache) abort();
  }
  cRefCache[cRefN++] = r;
  return r;
}

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
        for (int k = 0; k < t->ntypeParams && !seen; k++) seen = t->typeParams[k]->symbol == tp->symbol;
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
  sym->declaredType = t;
  return t;
}

/* a type reference to a symbol with type arguments */
static TsType *cTypeFromSymbolReference(TsSymbol *s, TsNode *ref) {
  s = cGetMergedSymbol(s);
  if (s->flags & SF_TypeAlias) {
    if (ref->typeArguments && ref->typeArguments->n) return cUnported("generic alias reference");
    if (cGetMergedSymbol(s)->decls[0]->typeParameters && cGetMergedSymbol(s)->decls[0]->typeParameters->n) return cUnported("generic alias");
    return cDeclaredTypeOfTypeAlias(s);
  }
  if (s->flags & (SF_Class | SF_Interface)) {
    TsType *d = cDeclaredTypeOfClassOrInterface(s);
    int na = ref->typeArguments ? ref->typeArguments->n : 0;
    if (d->ntypeParams == 0 && na == 0) return d;
    if (na != d->ntypeParams) return cUnported("type argument count");
    TsType **args = ts_arena_alloc(sizeof(TsType *) * (size_t)(na ? na : 1));
    for (int i = 0; i < na; i++) {
      args[i] = cTypeFromNode(ref->typeArguments->v[i]);
      if (args[i] == cErrorType) return cErrorType;
    }
    return cTypeReference(d, args, na);
  }
  if (s->flags & SF_TypeParameter) return cTypeOfTypeParameter(s);
  return cUnported("type reference kind");
}

/* the symbol a TypeReference names (type meaning), following import aliases */
static TsSymbol *cSymbolOfTypeName(TsNode *loc, TsNode *name) {
  if (name->kind != TSK_Identifier) return NULL;
  TsSymbol *s = cResolveTypeName(loc, bnEscapeText(name->text, name->textLen));
  if (s && (s->flags & SF_Alias)) s = cResolveImportAlias(s, 0);
  return s;
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
static TsType *cUniqueSymbolType(TsNode *decl) {
  if (decl->kind != TSK_PropertySignature || !(bModifierFlags(decl) & TMF_Readonly) || !decl->symbol)
    return cUnported("unique symbol placement");
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

/* getTypeFromTypeNode (67684) for the node kinds of this project */
static TsType *cTypeFromNode(TsNode *n) {
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
      TsType **members = NULL;
      int nm = 0, cap = 0;
      for (int i = 0; n->types && i < n->types->n; i++) {
        TsType *m = cTypeFromNode(n->types->v[i]);
        if (m == cErrorType) return cErrorType;
        /* flatten nested unions and intersections of the same kind */
        TsType **parts = &m;
        int np = 1;
        if (m->flags & fl) { parts = m->types; np = m->ntypes; }
        for (int k = 0; k < np; k++) {
          bool dup = false;
          for (int j = 0; j < nm && !dup; j++) dup = members[j] == parts[k];
          if (dup) continue;
          if (nm == cap) {
            cap = cap ? cap * 2 : 8;
            members = realloc(members, sizeof(TsType *) * (size_t)cap);
            if (!members) abort();
          }
          members[nm++] = parts[k];
        }
      }
      if (nm == 1 && fl == TF_Union) { TsType *only = members[0]; free(members); return only; }
      TsType *u = cNewType(fl);
      u->types = ts_arena_alloc(sizeof(TsType *) * (size_t)(nm ? nm : 1));
      memcpy(u->types, members, sizeof(TsType *) * (size_t)nm);
      u->ntypes = nm;
      free(members);
      return u;
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

/* resolved members of an object type: the declared members, then inherited members of the base types */
static void cResolveMembers(TsType *t) {
  if (t->resolved) return;
  t->resolved = true;
  if (t->objectFlags & OF_Reference) return;
  if (t->typeNode && (t->typeNode->kind == TSK_TypeLiteral)) {
    t->members = cMembersWithLate(t->symbol, t->symbol->members);
    return;
  }
  if (t->objectFlags & (OF_Interface | OF_Class)) {
    TsSymbol *s = t->symbol;
    TsSymTable *own = cMembersWithLate(s, s->members ? s->members : bTableNew());
    /* base types: heritage clauses of the declarations (interface extends, class extends) */
    TsSymTable *res = cTableCopy(own);
    for (int d = 0; d < s->ndecls; d++) {
      TsNode *dn = s->decls[d];
      for (int h = 0; dn->heritageClauses && h < dn->heritageClauses->n; h++) {
        TsNode *hc = dn->heritageClauses->v[h];
        if (hc->extra != TSK_ExtendsKeyword) continue;
        for (int e = 0; hc->types && e < hc->types->n; e++) {
          TsNode *ewta = hc->types->v[e];
          if (ewta->typeArguments && ewta->typeArguments->n) cUnported("generic base type");
          TsNode *expr = ewta->expression;
          if (!expr || expr->kind != TSK_Identifier) { cUnported("base type expression"); continue; }
          TsSymbol *bs = cSymbolOfTypeName(dn, expr);
          if (!bs) { cUnported("unresolved base"); continue; }
          TsType *bt = cTypeFromSymbolReference(bs, ewta);
          if (bt == cErrorType) continue;
          cResolveMembers(bt);
          if (!bt->members) continue;
          for (int i = 0; i < bt->members->n; i++) {
            BName key = bt->members->e[i].key;
            if (cIsSignatureName(key)) continue;
            if (bTableIndex(res, key) < 0) bTableSet(res, key, bt->members->e[i].sym);
          }
        }
      }
    }
    t->members = res;
    return;
  }
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
 * function or method symbol, or the type node of an anonymous function type */
static void cResolveSignatures(TsType *t) {
  if (t->sigsResolved) return;
  t->sigsResolved = true;
  cResolveMembers(t);
  TsSymbol *callSym = NULL, *newSym = NULL;
  if (t->typeNode && (t->typeNode->kind == TSK_FunctionType || t->typeNode->kind == TSK_ConstructorType)) {
    bool ctor = t->typeNode->kind == TSK_ConstructorType;
    t->call = ts_arena_alloc(sizeof(CSig));
    t->call[0].decl = t->typeNode;
    t->call[0].isConstruct = ctor;
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
      t->ncall++;
    }
  }
  if (newSym) {
    t->construct = ts_arena_alloc(sizeof(CSig) * (size_t)newSym->ndecls);
    for (int d = 0; d < newSym->ndecls; d++) {
      t->construct[t->nconstruct].decl = newSym->decls[d];
      t->construct[t->nconstruct].isConstruct = true;
      t->nconstruct++;
    }
  }
}

/* index infos: the __index declarations with one parameter */
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
    ii.keyType = cTypeFromNode(decl->parameters->v[0]->type);
    ii.type = cTypeFromNode(decl->type);
    ii.isReadonly = (bModifierFlags(decl) & TMF_Readonly) != 0;
    t->index[t->nindex++] = ii;
  }
}

/* ---- types of symbols (getTypeOfSymbol) ---- */
static TsType *cTypeOfSymbol(TsSymbol *sym) {
  sym = cGetMergedSymbol(sym);
  if (sym->type) return sym->type;
  TsType *t = NULL;
  if (sym->flags & SF_Alias) {
    TsSymbol *target = cResolveImportAlias(sym, 0);
    t = target ? cTypeOfSymbol(target) : cErrorType;
  } else if (sym->flags & SF_Variable) {
    TsNode *d = sym->valueDeclaration ? sym->valueDeclaration : sym->decls[0];
    if (d->kind != TSK_VariableDeclaration || !d->type || d->initializer) t = cUnported("variable without annotation");
    else t = cTypeFromNode(d->type);
  } else if (sym->flags & SF_Property) {
    TsNode *d = sym->valueDeclaration ? sym->valueDeclaration : sym->decls[0];
    if (d->kind != TSK_PropertySignature || !d->type) t = cUnported("property without annotation");
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
static TsType *cParamTypeAt(CParams *cp, int pos, bool *ok) {
  TsNode *p = pos < cp->n ? cp->params[pos] : cp->rest;
  *ok = true;
  if (!p) { *ok = false; return cErrorType; }
  if (p->dotDotDotToken) {
    TsNode *tn = p->type;
    if (tn && tn->kind == TSK_ArrayType) return cTypeFromNode(tn->elementType);
    if (tn && tn->kind == TSK_TypeReference && tn->typeArguments && tn->typeArguments->n == 1) return cTypeFromNode(tn->typeArguments->v[0]);
    return cUnported("rest parameter element type");
  }
  TsType *t = cTypeFromNode(p->type);
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
  if (a->decl == b->decl) return true;
  TsNode *da = a->decl, *db = b->decl;
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
    TsType *ta = cTypeFromNode(pa.thisParam->type);
    if (pb.thisParam) {
      TsType *tb = cTypeFromNode(pb.thisParam->type);
      if (ta == cErrorType || tb == cErrorType) { cUnported("this parameter"); return true; }
      if (!cIdentical(ta, tb)) return false;
    }
  }
  /* parameter types at every position of the target */
  for (int i = 0; i < pb.n; i++) {
    bool okA, okB;
    TsType *tx = cParamTypeAt(&pa, i, &okA), *ty = cParamTypeAt(&pb, i, &okB);
    if (!okA || !okB || tx == cErrorType || ty == cErrorType) { cUnported("parameter type"); return true; }
    if (!cIdentical(ty, tx)) return false;
  }
  /* return types */
  if ((da->type && da->type->kind == TSK_TypePredicate) || (db->type && db->type->kind == TSK_TypePredicate)) {
    cUnported("type predicate");
    return true;
  }
  TsType *ra = da->type ? cTypeFromNode(da->type) : cAnyType;
  TsType *rb = db->type ? cTypeFromNode(db->type) : cAnyType;
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

/* isTypeIdenticalTo: the entry point used by the checks */
static bool cIsTypeIdenticalTo(TsType *s, TsType *t) { return cIdentical(s, t); }

#endif
