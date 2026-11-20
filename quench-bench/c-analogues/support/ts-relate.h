/*
 * Checker port, relations (TypeScript 5.9.2 checker.ts; line numbers in lib/typescript.js): the
 * assignable, subtype and strict subtype relations (isTypeRelatedTo 69, checkTypeRelatedTo, isRelatedTo,
 * recursiveTypeRelatedTo with the maybe stack, the source and target stacks and the expanding check,
 * structuredTypeRelatedTo, unionOrIntersectionRelatedTo, propertiesRelatedTo, signaturesRelatedTo,
 * compareSignaturesRelated, indexSignaturesRelatedTo, variances of generic types by marker types
 * (getVariancesWorker) and typeArgumentsRelatedTo), the simple rules (isSimpleTypeRelatedTo), the
 * weak type check, the apparent types of primitives, and the relation caches keyed like getRelationKey
 * (source id, target id, intersection state). The identity relation is support/ts-types.h (cIdentical).
 *
 * Return types of signatures without an annotation are inferred from the return statements of the body
 * (getReturnTypeFromBody) for the expression forms that are ported: this, identifiers, property accesses,
 * calls of functions with one signature, `as` assertions and literals (widened). The union of the
 * return types is not reduced by subtypes.
 *
 * NOT PORTED (a relation that meets one sets cUnportedHit and is answered as related, so the caller
 * cannot report a false error): intersections as sources with structural comparison of their combined
 * members, enum types, template literals, mapped, conditional, indexed access and
 * keyof types, generic signatures (a comparison with type parameters), rest parameters at a position
 * that is not the last one, callback parameters (single call signature parameters are compared plainly),
 * the optional-property-in-class-member error (2327), private and protected members, abstract
 * constructors, type predicates, and the discriminant and object literal freshness rules.
 */
#ifndef TS_RELATE_H
#define TS_RELATE_H

#include "ts-types.h"

/* the relation enums (REL_, RR_, IS_, VF_) are declared in ts-types.h, before the conditional types use them */

typedef struct { TsType *s, *t; uint8_t st; uint8_t ok; } CRelEnt;
typedef struct { CRelEnt *e; int n, cap; int32_t *slot; int slotCap; } CRelTab;
static CRelTab cRelTab[REL_Count];
static int cCurRel = REL_Assignable;     /* the relation of the comparisons in progress */
static int cRelDepth;                    /* isRelatedTo nesting (for the marker-variance computation) */

/* the maybe stack (maybeKeys) and the source and target stacks of the expanding check */
typedef struct { TsType *s, *t; uint8_t st; } CMaybeKey;
static CMaybeKey cMaybeKeys[256];
static int cMaybeN;
static TsType *cSrcStack[128], *cTgtStack[128];
static int cSrcDepth, cTgtDepth;
static int cRelOverflows;                /* comparisons cut off by the depth limits */

/* marker types of the variance computation (markerSuperType, markerSubType, markerOtherType) */
static TsType *cMarkerSuper, *cMarkerSub, *cMarkerOther;
static TsType *cGlobalStructTypes[5];   /* String, Number, Boolean, BigInt, Symbol: declared types (lazy) */
/* anyFunctionType: the placeholder of a context-sensitive function argument while a candidate is checked (checkMode
 * SkipContextSensitive). It has no members; signaturesRelatedTo relates it to any signatures (signaturesRelatedTo, 71400s). */
static TsType *cAnyFunctionType;

static int cIsRelatedTo(TsType *s, TsType *t, int st);
static bool cIsRelatedWith(TsType *s, TsType *t, int rel);

/* ---- relation caches (relation.get / relation.set; the key is the two type ids and the state) ---- */
static uint32_t cRelHash(TsType *s, TsType *t, int st) {
  uint32_t h = (uint32_t)s->id * 2654435761u ^ (uint32_t)t->id * 40503u ^ (uint32_t)st * 97u;
  return h ^ (h >> 15);
}

static int cRelFind(CRelTab *tab, TsType *s, TsType *t, int st) {
  if (!tab->slotCap) return -1;
  uint32_t mask = (uint32_t)tab->slotCap - 1, h = cRelHash(s, t, st) & mask;
  for (;;) {
    int32_t v = tab->slot[h];
    if (v == 0) return -1;
    CRelEnt *e = &tab->e[v - 1];
    if (e->s == s && e->t == t && e->st == st) return v - 1;
    h = (h + 1) & mask;
  }
}

/* debug hook (typescript-lib --trace-iface): called for each new relation cache entry */
static void (*cRelStoreHook)(int rel, TsType *s, TsType *t, bool ok);

static void cRelStore(CRelTab *tab, TsType *s, TsType *t, int st, bool ok) {
  if (cRelStoreHook) cRelStoreHook((int)(tab - cRelTab), s, t, ok);
  if (tab->n == tab->cap) {
    tab->cap = tab->cap ? tab->cap * 2 : 1024;
    tab->e = realloc(tab->e, sizeof(CRelEnt) * (size_t)tab->cap);
    if (!tab->e) abort();
  }
  if ((tab->n + 1) * 2 > tab->slotCap) {
    int ns = tab->slotCap ? tab->slotCap * 2 : 2048;
    free(tab->slot);
    tab->slot = calloc((size_t)ns, sizeof(int32_t));
    if (!tab->slot) abort();
    tab->slotCap = ns;
    for (int i = 0; i < tab->n; i++) {
      uint32_t mask = (uint32_t)ns - 1, h = cRelHash(tab->e[i].s, tab->e[i].t, tab->e[i].st) & mask;
      while (tab->slot[h]) h = (h + 1) & mask;
      tab->slot[h] = i + 1;
    }
  }
  tab->e[tab->n].s = s;
  tab->e[tab->n].t = t;
  tab->e[tab->n].st = (uint8_t)st;
  tab->e[tab->n].ok = ok;
  tab->n++;
  uint32_t mask = (uint32_t)tab->slotCap - 1, h = cRelHash(s, t, st) & mask;
  while (tab->slot[h]) h = (h + 1) & mask;
  tab->slot[h] = tab->n;
}

static void cResetRelationCaches(void) {
  for (int r = 0; r < REL_Count; r++) {
    cRelTab[r].n = 0;
    free(cRelTab[r].slot);
    cRelTab[r].slot = NULL;
    cRelTab[r].slotCap = 0;
  }
  cMaybeN = 0;
  cSrcDepth = cTgtDepth = 0;
  cRelOverflows = 0;
  cRelDepth = 0;
}

/* ---- the types the relation looks at ---- */
static bool cIsStructured(TsType *t) { return (t->flags & (TF_Object | TF_Union | TF_Intersection | TF_TypeParameter | TF_Index | TF_IndexedAccess)) != 0; }

/* getConstraintOfTypeParameter: the declared constraint (NULL when unconstrained); the marker sub type
 * is constrained by the marker super type */
static TsType *cConstraintOf(TsType *tp) {
  if (tp->tpConstState == 2) return tp->tpConstraint;
  if (tp->tpConstState == 1) return NULL;   /* circular constraint */
  tp->tpConstState = 1;
  TsType *c = NULL;
  if (tp == cMarkerSub) c = cMarkerSuper;
  else if (tp->symbol) {
    TsSymbol *sym = tp->symbol;
    for (int d = 0; d < sym->ndecls && !c; d++)
      if (sym->decls[d]->kind == TSK_TypeParameter && sym->decls[d]->constraint) c = cTypeFromNode(sym->decls[d]->constraint);
  }
  tp->tpConstraint = c;
  tp->tpConstState = 2;
  return c;
}

/* getApparentType of a primitive: the global interface of its wrapper (String, Number, ...) */
static TsType *cApparentType(TsType *s) {
  int idx;
  const char *name;
  if (s->flags & (TF_String | TF_StringLiteral | TF_TemplateLiteral | TF_StringMapping)) { idx = 0; name = "String"; }
  else if (s->flags & (TF_Number | TF_NumberLiteral | TF_Enum)) { idx = 1; name = "Number"; }
  else if (s->flags & (TF_Boolean | TF_BooleanLiteral)) { idx = 2; name = "Boolean"; }
  else if (s->flags & TF_BigInt) { idx = 3; name = "BigInt"; }
  else if (s->flags & (TF_ESSymbol | TF_UniqueESSymbol)) { idx = 4; name = "Symbol"; }
  else return NULL;
  if (!cGlobalStructTypes[idx]) {
    TsSymbol *g = bTableGet(cGlobals, bnC(name));
    if (!g) return NULL;
    cGlobalStructTypes[idx] = cDeclaredTypeOfClassOrInterface(g);
  }
  return cGlobalStructTypes[idx];
}

/* isSimpleTypeRelatedTo (checker.ts): the rules of the primitive, any, unknown and never types */
static bool cSameLiteralValue(TsType *s, TsType *t) {
  if (s->flags & TF_NumberLiteral) return s->litNum == t->litNum;
  return s->textLen == t->textLen && memcmp(s->text, t->text, (size_t)s->textLen) == 0;
}

/* isUnitType (Unit = Enum | Literal | UniqueESSymbol | Nullable) */
static bool cIsUnitType(TsType *t) {
  return (t->flags & (TF_Enum | TF_StringLiteral | TF_NumberLiteral | TF_BigIntLiteral | TF_BooleanLiteral | TF_UniqueESSymbol |
                      TF_Undefined | TF_Null)) != 0;
}

/* isEmptyResolvedType: an object type with no properties, signatures or index infos */
static bool cIsEmptyResolved(TsType *t) {
  if (!(t->flags & TF_Object)) return false;
  TsSymbol **props;
  if (cPropertiesOf(t, &props) != 0) return false;
  cResolveSignatures(t);
  cResolveIndexInfos(t);
  return t->ncall == 0 && t->nconstruct == 0 && t->nindex == 0;
}

/* isEmptyAnonymousObjectType: an anonymous object type that is empty */
static bool cIsEmptyAnonymousObject(TsType *t) {
  return (t->flags & TF_Object) && (t->objectFlags & OF_Anonymous) && cIsEmptyResolved(t);
}

/* isUnknownLikeUnionType (strictNullChecks): undefined | null | {} and more members, with the empty object type */
static bool cIsUnknownLikeUnion(TsType *t) {
  if (!(t->flags & TF_Union) || t->ntypes < 3) return false;
  if (!(t->types[0]->flags & TF_Undefined) || !(t->types[1]->flags & TF_Null)) return false;
  for (int i = 0; i < t->ntypes; i++) if (cIsEmptyAnonymousObject(t->types[i])) return true;
  return false;
}

static bool cIsSimpleRelated(TsType *s, TsType *t, int rel) {
  uint32_t sf = s->flags, tf = t->flags;
  if ((tf & (TF_Any | TF_Unknown)) || (sf & TF_Never)) return true;
  if ((sf & TF_Undefined) && (tf & (TF_Undefined | TF_Void))) return true;   /* strictNullChecks */
  if ((sf & TF_Null) && (tf & TF_Null)) return true;
  if ((sf & TF_Object) && (tf & TF_NonPrimitive) && !(rel == REL_StrictSubtype && (s->objectFlags & OF_Anonymous) && !s->symbol)) return true;
  /* isSimpleTypeRelatedTo (assignable or comparable): any, the number and enum rules, unknown-like unions */
  bool assignOrComparable = rel == REL_Assignable || rel == REL_Comparable;
  if (assignOrComparable && (sf & TF_Any)) return true;
  if ((sf & (TF_String | TF_StringLiteral)) && (tf & TF_String)) return true;
  /* enum literals (isSimpleTypeRelatedTo, 5.9.2): a literal of an enum relates to the plain literal of its value */
  if ((sf & TF_StringLiteral) && (sf & TF_EnumLiteral) && (tf & TF_StringLiteral) && !(tf & TF_EnumLiteral) && cSameLiteralValue(s, t)) return true;
  if ((sf & (TF_Number | TF_NumberLiteral | TF_Enum)) && (tf & TF_Number)) return true;
  if ((sf & TF_NumberLiteral) && (sf & TF_EnumLiteral) && (tf & TF_NumberLiteral) && !(tf & TF_EnumLiteral) && cSameLiteralValue(s, t)) return true;
  if ((sf & TF_EnumLiteral) && (tf & TF_EnumLiteral)) {
    /* isEnumTypeRelatedTo: the same enum relates; two different enums of one name would need the members compared */
    if (((sf & TF_Union) && (tf & TF_Union)) && s->symbol == t->symbol) return true;
    if (((sf & TF_Union) && (tf & TF_Union)) && s->symbol && t->symbol && s->symbol->name.n == t->symbol->name.n &&
        memcmp(s->symbol->name.s, t->symbol->name.s, (size_t)s->symbol->name.n) == 0) {
      cUnported("enum union relation between two enums of one name");
      return true;
    }
  }
  if ((sf & TF_Enum) && (tf & TF_Enum)) {
    if (s->symbol == t->symbol) return true;
    if (s->symbol && t->symbol && s->symbol->name.n == t->symbol->name.n &&
        memcmp(s->symbol->name.s, t->symbol->name.s, (size_t)s->symbol->name.n) == 0) {
      cUnported("enum relation between two enums of one name");
      return true;
    }
  }
  if (assignOrComparable) {
    if ((sf & TF_Number) && ((tf & TF_Enum) || ((tf & TF_NumberLiteral) && (tf & TF_EnumLiteral)))) return true;
    if ((sf & TF_NumberLiteral) && !(sf & TF_EnumLiteral) &&
        ((tf & TF_Enum) || ((tf & TF_NumberLiteral) && (tf & TF_EnumLiteral) && s->litNum == t->litNum))) return true;
    if (cIsUnknownLikeUnion(t)) return true;
  }
  if ((sf & TF_BigInt) && (tf & TF_BigInt)) return true;
  if ((sf & (TF_Boolean | TF_BooleanLiteral)) && (tf & TF_Boolean)) return true;
  if ((sf & (TF_ESSymbol | TF_UniqueESSymbol)) && (tf & TF_ESSymbol)) return true;
  return false;
}

/* isWeakType: an object type with properties, all optional, and no signatures or index signatures */
static bool cIsWeakType(TsType *t) {
  /* isWeakType (71764): the structured members of an object are all resolved (signatures and index infos too); an
   * intersection is weak when every member is */
  if (t->flags & TF_Object) {
    TsSymbol **props;
    int n = cPropertiesOf(t, &props);
    cResolveSignatures(t);
    cResolveIndexInfos(t);
    if (t->ncall || t->nconstruct || t->nindex || n == 0) return false;
    for (int i = 0; i < n; i++) if (!(props[i]->flags & SF_Optional)) return false;
    return true;
  }
  if (t->flags & TF_Intersection) {
    for (int i = 0; i < t->ntypes; i++) if (!cIsWeakType(t->types[i])) return false;
    return true;
  }
  return false;
}

/* isKnownProperty (for the common property check): an object's property, or one of the members of an intersection */
static bool cIsKnownProperty(TsType *t, BName name) {
  if (t->flags & TF_Intersection) {
    for (int i = 0; i < t->ntypes; i++) if (cIsKnownProperty(t->types[i], name)) return true;
    return false;
  }
  return (t->flags & TF_Object) && cPropertyOf(t, name) != NULL;
}

static bool cHasCommonProperties(TsType *s, TsType *t) {
  TsSymbol **props;
  int n = cPropertiesOf(s, &props);
  for (int i = 0; i < n; i++) if (cIsKnownProperty(t, props[i]->name)) return true;
  return false;
}

/* ---- return types ---- */
static TsType *cReturnTypeOf(CSig *sig);
static TsType *cExprType(TsNode *e);

/* the class declared type enclosing a node (the type of `this` in a class member) */
static TsType *cEnclosingClassType(TsNode *n) {
  for (TsNode *p = n->parent; p; p = p->parent) {
    if ((p->kind == TSK_ClassDeclaration || p->kind == TSK_ClassExpression) && p->symbol)
      return cDeclaredTypeOfClassOrInterface(cGetMergedSymbol(p->symbol));
    if (p->kind == TSK_SourceFile) break;
  }
  return NULL;
}

/* the return statements of a body, not those of nested functions */
static void cCollectReturns(TsNode *n, TsNode ***out, int *cnt, int *cap) {
  if (!n) return;
  switch (n->kind) {
    case TSK_FunctionDeclaration: case TSK_FunctionExpression: case TSK_ArrowFunction: case TSK_MethodDeclaration:
    case TSK_Constructor: case TSK_GetAccessor: case TSK_SetAccessor: case TSK_ClassDeclaration: case TSK_ClassExpression:
      return;
    case TSK_ReturnStatement:
      if (*cnt == *cap) {
        *cap = *cap ? *cap * 2 : 8;
        *out = realloc(*out, sizeof(TsNode *) * (size_t)*cap);
      }
      (*out)[(*cnt)++] = n;
      return;
    default: break;
  }
  if (n->statements) for (int i = 0; i < n->statements->n; i++) cCollectReturns(n->statements->v[i], out, cnt, cap);
  if (n->clauses) for (int i = 0; i < n->clauses->n; i++) cCollectReturns(n->clauses->v[i], out, cnt, cap);
  cCollectReturns(n->thenStatement, out, cnt, cap);
  cCollectReturns(n->elseStatement, out, cnt, cap);
  cCollectReturns(n->statement, out, cnt, cap);
  cCollectReturns(n->block, out, cnt, cap);
  cCollectReturns(n->finallyBlock, out, cnt, cap);
  cCollectReturns(n->catchClause, out, cnt, cap);
  cCollectReturns(n->caseBlock, out, cnt, cap);
}

static TsType *cBaseConstraintOf(TsType *t);

/* removeSubtypes (65855, UnionReduction.Subtype of getUnionType). The members are scanned from the last one; a member
 * is a candidate when it is structured or instantiable, or when an empty object type is a member. A candidate is
 * dropped when it is a strict subtype of another member, and the targets whose key property (the first unit-typed
 * property of the candidate) has another unit type are not compared. The subtype reduction cache is not kept. */
static TsType *cUnionWithSubtypesRemoved(TsType **ts, int n) {
  if (n == 0) return cNeverType;
  if (n == 1) return ts[0];
  bool hasObject = false;
  for (int i = 0; i < n; i++) if (ts[i]->flags & TF_Object) hasObject = true;
  bool hasEmptyObject = false;
  for (int i = 0; hasObject && i < n; i++) {
    TsType *m = ts[i];
    if ((m->flags & TF_Object) && !(m->objectFlags & OF_Mapped) && cIsEmptyResolved(m)) hasEmptyObject = true;
  }
  bool *alive = calloc((size_t)n, sizeof(bool));
  if (!alive) abort();
  for (int i = 0; i < n; i++) alive[i] = true;
  /* removeRedundantLiteralTypes (reduceVoidUndefined, the subtype reduction): a literal of a primitive that is a member,
   * a unique symbol with es symbol, and undefined with void are dropped first */
  bool hasStr = false, hasNum = false, hasBig = false, hasESS = false, hasVoid = false;
  for (int i = 0; i < n; i++) {
    uint32_t f = ts[i]->flags;
    hasStr |= (f & TF_String) != 0; hasNum |= (f & TF_Number) != 0; hasBig |= (f & TF_BigInt) != 0;
    hasESS |= (f & TF_ESSymbol) != 0; hasVoid |= (f & TF_Void) != 0;
  }
  for (int i = 0; i < n; i++) {
    uint32_t f = ts[i]->flags;
    if (((f & TF_StringLiteral) && hasStr) || ((f & TF_NumberLiteral) && hasNum) || ((f & TF_BigIntLiteral) && hasBig) ||
        ((f & TF_UniqueESSymbol) && hasESS) || ((f & TF_Undefined) && hasVoid))
      alive[i] = false;
  }
  for (int i = n - 1; i >= 0; i--) {
    if (!alive[i]) continue;
    TsType *source = ts[i];
    if (!(hasEmptyObject || (source->flags & (TF_Object | TF_Union | TF_Intersection | TF_TypeParameter | TF_Index |
                                              TF_IndexedAccess | TF_Conditional | TF_TemplateLiteral | TF_StringMapping))))
      continue;
    if (source->flags & TF_TypeParameter) {
      /* a type parameter whose constraint is a union is dropped when it is a strict subtype of the other members */
      TsType *bc = cBaseConstraintOf(source);
      if (bc == cErrorType) { cUnported("subtype reduction of a type parameter"); continue; }
      if (bc && (bc->flags & TF_Union)) {
        TsType **others = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
        int no = 0;
        for (int j = 0; j < n; j++) if (alive[j]) others[no++] = ts[j] == source ? cNeverType : ts[j];
        TsType *u = no == 1 ? others[0] : cGetUnionType(others, no);
        if (cIsRelatedWith(source, u, REL_StrictSubtype)) alive[i] = false;
        continue;
      }
    }
    /* the key property: the first property of the candidate with a unit type */
    TsSymbol *keyProp = NULL;
    TsType *keyType = NULL;
    if (source->flags & (TF_Object | TF_Intersection | TF_TypeParameter | TF_IndexedAccess | TF_Conditional)) {
      if (source->flags & TF_Object) {
        TsSymbol **props;
        int np = cPropertiesOf(source, &props);
        for (int p = 0; p < np && !keyProp; p++) {
          TsType *pt = cTypeOfSymbol(props[p]);
          if (cIsUnitType(pt)) { keyProp = props[p]; keyType = cRegularType(pt); }
        }
      } else {
        cUnported("subtype reduction key property of an intersection or instantiable type");
        continue;
      }
    }
    for (int j = 0; j < n; j++) {
      if (j == i || !alive[j]) continue;
      TsType *target = ts[j];
      if (keyProp && (target->flags & (TF_Object | TF_Intersection | TF_TypeParameter | TF_IndexedAccess | TF_Conditional))) {
        if (!(target->flags & TF_Object)) { cUnported("subtype reduction key property of an intersection or instantiable target"); continue; }
        TsSymbol *tp = cPropertyOf(target, keyProp->name);
        if (tp) {
          TsType *tt = cTypeOfSymbol(tp);
          if (cIsUnitType(tt) && cRegularType(tt) != keyType) continue;
        }
      }
      if (!cIsRelatedWith(source, target, REL_StrictSubtype)) continue;
      /* a class source and a class target are kept unless the source derives from the target (not ported) */
      bool bothClasses = (source->objectFlags & OF_Class) && (target->objectFlags & OF_Class);
      if (bothClasses) { cUnported("subtype reduction between classes"); continue; }
      alive[i] = false;
      break;
    }
  }
  TsType **kept = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
  int k = 0;
  for (int i = 0; i < n; i++) if (alive[i]) kept[k++] = ts[i];
  free(alive);
  return k == 1 ? kept[0] : cGetUnionType(kept, k);
}

/* a union or a literal type with a literal member */
static bool cHasLiteralMember(TsType *t) {
  if (t->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BooleanLiteral | TF_BigIntLiteral)) return true;
  if (t->flags & TF_Union) for (int i = 0; i < t->ntypes; i++) if (cHasLiteralMember(t->types[i])) return true;
  return false;
}

/* getReturnTypeOfSignature for a declaration without a return annotation (uninstantiated) */
static TsType *cInferReturnType(TsNode *d) {
  if (d->inferState == 2) return d->inferredReturn;
  if (d->inferState == 1) return cUnported("circular return type");
  d->inferState = 1;
  TsType *r;
  if (!d->body) r = cAnyType;
  else if (d->asteriskToken || (bModifierFlags(d) & TMF_Async)) r = cUnported("inferred return of an async or generator function");
  else if (d->body->kind != TSK_Block) {
    /* an arrow function with an expression body: the type of the expression, widened as a return (getReturnTypeFromBody) */
    TsType *t = cExprType(d->body);
    if (t != cErrorType && cHasLiteralMember(t)) d->retLiteral = 1;
    if (t->flags & TF_StringLiteral) t = cStringType;
    else if (t->flags & TF_NumberLiteral) t = cNumberType;
    else if (t->flags & TF_BooleanLiteral) t = cBooleanType;
    r = t;
  } else {
    TsNode **rets = NULL;
    int nr = 0, cap = 0;
    cCollectReturns(d->body, &rets, &nr, &cap);
    if (nr == 0) r = cVoidType;
    else {
      TsType **ts = ts_arena_alloc(sizeof(TsType *) * (size_t)nr);
      int k = 0;
      for (int i = 0; i < nr; i++) {
        TsType *t = rets[i]->expression ? cExprType(rets[i]->expression) : cUndefinedType;
        if (t == cErrorType) { k = -1; break; }
        if (cHasLiteralMember(t)) d->retLiteral = 1;
        /* widening of the literal types of a return (getWidenedLiteralLikeType) */
        if (t->flags & TF_StringLiteral) t = cStringType;
        else if (t->flags & TF_NumberLiteral) t = cNumberType;
        else if (t->flags & (TF_BooleanLiteral)) t = cBooleanType;
        ts[k++] = t;
      }
      free(rets);
      r = k < 0 ? cErrorType : (nr == 1 ? ts[0] : cUnionWithSubtypesRemoved(ts, nr));
    }
  }
  d->inferredReturn = r;
  d->inferState = 2;
  return r;
}

/* getReturnTypeOfSignature: the return type of an instance is resolved once (retCell); an error is not kept */
static TsType *cReturnTypeOfUncached(CSig *sig);
static TsType *cReturnTypeOf(CSig *sig) {
  if (sig->retCell && *sig->retCell) return *sig->retCell;
  TsType *r = cReturnTypeOfUncached(sig);
  if (sig->retCell && r != cErrorType) *sig->retCell = r;
  return r;
}
static TsType *cReturnTypeOfUncached(CSig *sig) {
  TsNode *d = sig->decl;
  if (d->kind == TSK_Constructor || (sig->isConstruct && !d->type)) {
    TsSymbol *cls = d->parent && d->parent->symbol ? cGetMergedSymbol(d->parent->symbol) : NULL;
    if (!cls) return cUnported("constructor without class");
    return cDeclaredTypeOfClassOrInterface(cls);
  }
  if (d->type && d->type->kind == TSK_TypePredicate) return cBooleanType;
  if (d->type) return cTypeFromNodeM(d->type, sig->mapper);
  TsType *r = cInferReturnType(d);
  if (r == cErrorType) return r;
  return sig->mapper ? cInstantiate(r, sig->mapper) : r;
}

/* ---- variances (getVariancesWorker, 66142; variance markers) ---- */
static uint8_t *cVariances(TsType *tg) {
  if (tg->varState == 2) return tg->variances;
  if (tg->varState == 1) return NULL;
  tg->varState = 1;
  int n = tg->ntypeParams;
  uint8_t *v = ts_arena_alloc((size_t)(n ? n : 1));
  TsType **sup = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  TsType **sub = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  TsType **oth = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  for (int i = 0; i < n; i++) {
    for (int k = 0; k < n; k++) { sup[k] = sub[k] = oth[k] = tg->typeParams[k]; }
    sup[i] = cMarkerSuper;
    sub[i] = cMarkerSub;
    oth[i] = cMarkerOther;
    TsType *withSuper = cTypeReference(tg, sup, n), *withSub = cTypeReference(tg, sub, n);
    uint8_t vi = 0;
    if (cIsRelatedTo(withSub, withSuper, IS_None) != RR_False) vi |= VF_Covariant;
    if (cIsRelatedTo(withSuper, withSub, IS_None) != RR_False) vi |= VF_Contravariant;
    if (vi == VF_Bivariant) {
      TsType *withOther = cTypeReference(tg, oth, n);
      if (cIsRelatedTo(withOther, withSuper, IS_None) != RR_False) vi = VF_Independent;
    }
    v[i] = vi;
  }
  tg->variances = v;
  tg->varState = 2;
  return v;
}

/* typeArgumentsRelatedTo (for references with the same target) */
static int cRelateVariances(TsType *s, TsType *t, uint8_t *v) {
  int result = RR_True;
  for (int i = 0; i < s->nargs; i++) {
    int r;
    /* typeArgumentsRelatedTo: the arguments past the type parameters (this) are covariant */
    uint8_t vi = i < s->target->ntypeParams ? v[i] : VF_Covariant;
    if (vi & VF_Independent) continue;
    TsType *sa = s->args[i], *ta = t->args[i];
    if (vi == VF_Covariant) r = cIsRelatedTo(sa, ta, IS_None);
    else if (vi == VF_Contravariant) r = cIsRelatedTo(ta, sa, IS_None);
    else if (vi == VF_Bivariant) {
      r = cIsRelatedTo(ta, sa, IS_None);
      if (r == RR_False) r = cIsRelatedTo(sa, ta, IS_None);
    } else {   /* invariant */
      r = cIsRelatedTo(sa, ta, IS_None);
      if (r != RR_False) { int r2 = cIsRelatedTo(ta, sa, IS_None); r = r2 < r ? r2 : r; }
    }
    if (r == RR_False) return RR_False;
    if (r < result) result = r;
  }
  return result;
}

/* ---- properties (propertiesRelatedTo, 68880) ---- */
static bool cIsTupleType(TsType *t) { return (t->objectFlags & OF_Tuple) != 0; }

static int cPropertiesRelated(TsType *s, TsType *t, int st) {
  (void)st;
  bool requireOptional = (cCurRel == REL_Subtype || cCurRel == REL_StrictSubtype) &&
                         !(s->objectFlags & OF_ObjectLiteral) && !cIsTupleType(s);
  TsSymbol **tp;
  int nt = cPropertiesOf(t, &tp);
  /* getUnmatchedProperty: a required target property the source lacks */
  for (int i = 0; i < nt; i++) {
    if (!(tp[i]->flags & SF_Optional) || requireOptional) {
      if (!cPropertyOf(s, tp[i]->name)) return RR_False;
    }
  }
  int result = RR_True;
  for (int i = 0; i < nt; i++) {
    TsSymbol *sp = cPropertyOf(s, tp[i]->name);
    if (!sp) continue;   /* an optional target property the source lacks */
    if (sp == tp[i]) continue;
    /* propertyRelatedTo: an optional source property is not assignable to a required target one */
    /* (propertyRelatedTo: skipOptional is set for the comparable relation) */
    if ((sp->flags & SF_Optional) && !(tp[i]->flags & SF_Optional) && cCurRel != REL_Identity && cCurRel != REL_Comparable) return RR_False;
    TsType *a = cTypeOfSymbol(sp), *b = cTypeOfSymbol(tp[i]);
    int r = cIsRelatedTo(a, b, IS_None);
    if (r == RR_False) return RR_False;
    if (r < result) result = r;
  }
  return result;
}

/* ---- signatures (compareSignaturesRelated, 70000s) ---- */
/* the rest parameter of a signature at its position: the array type of the rest parameter */
static TsType *cRestArrayAt(CParams *cp, TsMapper *m) {
  if (!cp->rest || !cp->rest->type) return cUnported("rest parameter without an array type");
  return cTypeFromNodeM(cp->rest->type, m);
}

static bool cCompareSignaturesRelated(CSig *src, CSig *tgt) {
  if (src->decl == tgt->decl && src->mapper == tgt->mapper) return true;
  TsNode *ds = src->decl, *dt = tgt->decl;
  CParams ps, pt;
  if (!cSplitParams(ds, &ps) || !cSplitParams(dt, &pt)) { cUnported("too many parameters"); return true; }
  /* sourceHasMoreParameters: the source needs more arguments than the target has */
  if (!pt.rest && ps.min > pt.n) return false;
  bool strictVariance;
  /* strictVariance: parameters are contravariant except for methods and constructors (strictFunctionTypes) */
  strictVariance = !(dt->kind == TSK_MethodDeclaration || dt->kind == TSK_MethodSignature || dt->kind == TSK_Constructor);
  int sCount = ps.n, tCount = pt.n;
  bool anyRest = ps.rest != NULL || pt.rest != NULL;
  int paramCount = anyRest ? (sCount < tCount ? sCount : tCount) : (sCount > tCount ? sCount : tCount);
  int restIndex = anyRest ? paramCount - 1 : -1;
  for (int i = 0; i < paramCount; i++) {
    TsType *st, *tt;
    if (i == restIndex) {
      st = cRestArrayAt(&ps, src->mapper);
      tt = cRestArrayAt(&pt, tgt->mapper);
      if (st == cErrorType || tt == cErrorType) continue;
    } else {
      bool okS, okT;
      st = cSigParamType(src, &ps, i, &okS);
      tt = cSigParamType(tgt, &pt, i, &okT);
      if (!okS || !okT) continue;
      if (st == cErrorType || tt == cErrorType) { cUnported("parameter type of a relation"); continue; }
    }
    if (st == tt) continue;
    /* compareTypes(sourceType, targetType) when the parameters are bivariant, then compareTypes(targetType, sourceType) */
    bool related = (!strictVariance && cIsRelatedTo(st, tt, IS_None) != RR_False) || cIsRelatedTo(tt, st, IS_None) != RR_False;
    if (!related) return false;
  }
  /* return types: a void or any target return type accepts any source return type */
  TsType *tr = cReturnTypeOf(tgt);
  if (tr == cVoidType || tr == cAnyType || tr == cErrorType) {
    if (tr == cErrorType) cUnportedHit = true;
    return true;
  }
  TsType *sr = cReturnTypeOf(src);
  if (sr == cErrorType) { cUnportedHit = true; return true; }
  return cIsRelatedTo(sr, tr, IS_None) != RR_False;
}

/* the type parameters of a signature declaration (the declared types of its type parameters) */
static TsType **cSigTypeParams(TsNode *d, int *n) {
  *n = d->typeParameters ? d->typeParameters->n : 0;
  TsType **tps = ts_arena_alloc(sizeof(TsType *) * (size_t)(*n ? *n : 1));
  for (int i = 0; i < *n; i++) tps[i] = cTypeOfTypeParameter(cGetMergedSymbol(d->typeParameters->v[i]->symbol));
  return tps;
}

static CSig *cGetSignatureInstance(CSig *base, TsType **args, int n);   /* ts-checker-body.h */
static bool cSigIsGeneric(CSig *sig) { return !sig->erased && sig->decl->typeParameters && sig->decl->typeParameters->n > 0; }

/* getErasedSignature (cached per signature): the type parameters instantiated to any */
typedef struct { TsNode *decl; TsMapper *mapper; CSig res; } CEraseEnt;
static CEraseEnt *cEraseList;
static int cEraseN, cEraseCap;
/* the erased signatures live in the checker arena: every checker run starts with none */
/* the base signatures of the generic instances (cBaseSignatureOf) live in the checker arena too */
typedef struct { TsNode *decl; TsMapper *mapper; TsType **vals; int n; bool fail; } CBaseEnt;
static CBaseEnt *cBaseList;
static int cBaseN, cBaseCap;
typedef struct { TsNode *decl; TsMapper *mapper; TsType *val; } CPredEnt;
static CPredEnt *cPredList;
static int cPredN, cPredCap;
static void cResetEraseList(void) { cEraseN = 0; cBaseN = 0; cPredN = 0; }
static CSig cErasedSignature(CSig *sig) {
  if (!cSigIsGeneric(sig)) return *sig;
  for (int i = 0; i < cEraseN; i++)
    if (cEraseList[i].decl == sig->decl && cEraseList[i].mapper == sig->mapper) return cEraseList[i].res;
  int n;
  TsType **tps = cSigTypeParams(sig->decl, &n);
  TsType **anys = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
  for (int i = 0; i < n; i++) anys[i] = cAnyType;
  CSig r = *sig;
  TsMapper *eraser = cCreateTypeMapper(tps, anys, n);
  r.mapper = cCombineMappers(sig->mapper, eraser);
  cSigInstantiate(&r, sig, eraser);   /* getErasedSignature: instantiateSignature(signature, eraser) */
  r.erased = true;
  if (cEraseN == cEraseCap) {
    cEraseCap = cEraseCap ? cEraseCap * 2 : 16;
    cEraseList = realloc(cEraseList, sizeof(CEraseEnt) * (size_t)cEraseCap);
    if (!cEraseList) abort();
  }
  cEraseList[cEraseN].decl = sig->decl;
  cEraseList[cEraseN].mapper = sig->mapper;
  cEraseList[cEraseN].res = r;
  cEraseN++;
  return r;
}

/* ---- inferTypes (checker.ts, 5.9.2) for the signature and conditional inferences ----
 * Ported: the identical and closely matched members of unions and intersections, the naked type variable of a
 * union target, references of one generic type (variances: contravariant arguments flip the direction),
 * indexed access pairs, source unions, the properties and call or construct signatures of object types (the
 * parameters contravariantly under strictFunctionTypes, the return types covariantly). Other shapes that could
 * hold type variables (index, conditional, template literal, string mapping, mapped types, rest parameters,
 * generic signatures, index signatures) are not ported and mark the request unported. */
static void cInferInit(CInfer *c, TsType **params, int n) {
  c->params = params;
  c->n = n;
  c->cand = ts_arena_zalloc(sizeof(TsType **) * (size_t)(n ? n : 1));
  c->con = ts_arena_zalloc(sizeof(TsType **) * (size_t)(n ? n : 1));
  c->cn = ts_arena_zalloc(sizeof(int) * (size_t)(n ? n : 1));
  c->cc = ts_arena_zalloc(sizeof(int) * (size_t)(n ? n : 1));
  c->ctn = ts_arena_zalloc(sizeof(int) * (size_t)(n ? n : 1));
  c->ctc = ts_arena_zalloc(sizeof(int) * (size_t)(n ? n : 1));
  c->prio = ts_arena_alloc(sizeof(int) * (size_t)(n ? n : 1));
  for (int i = 0; i < n; i++) c->prio[i] = -1;
  c->priority = 0;
  c->visS = c->visT = NULL; c->nvis = c->capvis = 0;
  c->contravariant = false;
  c->bivariant = false;
  c->hits = 0;
}

static int cInferIndexOf(CInfer *c, TsType *t) {
  if (!(t->flags & TF_TypeParameter)) return -1;
  for (int i = 0; i < c->n; i++) if (c->params[i] == t) return i;
  return -1;
}

static void cInferPush(TsType ***arr, int *cnt, int *cap, TsType *v) {
  if (*cnt == *cap) {
    int nc = *cap ? *cap * 2 : 4;
    TsType **nv = ts_arena_alloc(sizeof(TsType *) * (size_t)nc);
    if (*cnt) memcpy(nv, *arr, sizeof(TsType *) * (size_t)*cnt);
    *arr = nv;
    *cap = nc;
  }
  (*arr)[(*cnt)++] = v;
}

/* the candidate of a type variable (inferFromTypes, the TypeVariable branch): only candidates of the lowest
 * priority are kept, and a contravariant inference adds contravariant candidates */
static void cInferAddCandidate(CInfer *c, int k, TsType *source) {
  if (c->prio[k] < 0 || c->priority < c->prio[k]) {
    c->cn[k] = 0;
    c->ctn[k] = 0;
    c->prio[k] = c->priority;
  }
  if (c->priority != c->prio[k]) return;
  c->hits++;
  if (c->contravariant && !c->bivariant) {   /* inferFromTypes: bivariant (method) parameters are covariant candidates */
    for (int i = 0; i < c->ctn[k]; i++) if (c->con[k][i] == source) return;
    cInferPush(&c->con[k], &c->ctn[k], &c->ctc[k], source);
  } else {
    for (int i = 0; i < c->cn[k]; i++) if (c->cand[k][i] == source) return;
    cInferPush(&c->cand[k], &c->cn[k], &c->cc[k], source);
  }
}

static void cInferWithPriority(CInfer *c, TsType *source, TsType *target, int p) {
  int save = c->priority;
  c->priority |= p;
  cInferFrom(c, source, target);
  c->priority = save;
}

/* inferFromContravariantTypes under strictFunctionTypes (the project's tsconfig is strict) */
static void cInferContra(CInfer *c, TsType *source, TsType *target) {
  c->contravariant = !c->contravariant;
  cInferFrom(c, source, target);
  c->contravariant = !c->contravariant;
}

/* isTypeOrBaseIdenticalTo and isTypeCloselyMatchedBy (the matches of inferFromMatchingTypes) */
static bool cInferMatches(TsType *s, TsType *t, bool closely) {
  if (closely) return (s->flags & TF_Object) && (t->flags & TF_Object) && s->symbol && s->symbol == t->symbol;
  if (cIdentical(s, t)) return true;
  return ((t->flags & TF_String) && (s->flags & TF_StringLiteral)) || ((t->flags & TF_Number) && (s->flags & TF_NumberLiteral));
}

/* inferFromMatchingTypes: infers from each matching pair, then returns the unmatched members of both lists */
static void cInferMatching(CInfer *c, TsType ***srcs, int *ns, TsType ***tgts, int *nt, bool closely) {
  bool *sm = ts_arena_zalloc(sizeof(bool) * (size_t)(*ns ? *ns : 1));
  bool *tm = ts_arena_zalloc(sizeof(bool) * (size_t)(*nt ? *nt : 1));
  for (int ti = 0; ti < *nt; ti++)
    for (int si = 0; si < *ns; si++)
      if (cInferMatches((*srcs)[si], (*tgts)[ti], closely)) {
        cInferFrom(c, (*srcs)[si], (*tgts)[ti]);
        sm[si] = true;
        tm[ti] = true;
      }
  TsType **ks = ts_arena_alloc(sizeof(TsType *) * (size_t)(*ns ? *ns : 1));
  TsType **kt = ts_arena_alloc(sizeof(TsType *) * (size_t)(*nt ? *nt : 1));
  int cs = 0, ct = 0;
  for (int i = 0; i < *ns; i++) if (!sm[i]) ks[cs++] = (*srcs)[i];
  for (int i = 0; i < *nt; i++) if (!tm[i]) kt[ct++] = (*tgts)[i];
  *srcs = ks; *ns = cs; *tgts = kt; *nt = ct;
}

/* inferToMultipleTypes: a union target with one naked type variable takes the members that inferred nothing */
static void cInferToMultiple(CInfer *c, TsType *source, TsType *target) {
  int typeVarCount = 0;
  if (target->flags & TF_Union) {
    TsType *naked = NULL;
    TsType **sources = (source->flags & TF_Union) ? source->types : &source;
    int ns = (source->flags & TF_Union) ? source->ntypes : 1;
    bool *matched = ts_arena_zalloc(sizeof(bool) * (size_t)(ns ? ns : 1));
    for (int ti = 0; ti < target->ntypes; ti++) {
      TsType *t = target->types[ti];
      if (cInferIndexOf(c, t) >= 0) {
        naked = t;
        typeVarCount++;
      } else {
        for (int i = 0; i < ns; i++) {
          int h0 = c->hits;
          cInferFrom(c, sources[i], t);
          if (c->hits != h0) matched[i] = true;
        }
      }
    }
    if (typeVarCount == 0) return;
    if (typeVarCount == 1) {
      TsType **unmatched = ts_arena_alloc(sizeof(TsType *) * (size_t)(ns ? ns : 1));
      int nu = 0;
      for (int i = 0; i < ns; i++) if (!matched[i]) unmatched[nu++] = sources[i];
      if (nu) {
        cInferFrom(c, cGetUnionType(unmatched, nu), naked);
        return;
      }
    }
  } else {
    for (int ti = 0; ti < target->ntypes; ti++) {
      TsType *t = target->types[ti];
      if (cInferIndexOf(c, t) >= 0) typeVarCount++;
      else cInferFrom(c, source, t);
    }
  }
  if (target->flags & TF_Intersection ? typeVarCount == 1 : typeVarCount > 0) {
    for (int ti = 0; ti < target->ntypes; ti++)
      if (cInferIndexOf(c, target->types[ti]) >= 0) cInferWithPriority(c, source, target->types[ti], 1);
  }
}

/* inferFromTypeArguments: the arguments of a reference, contravariant ones with the flipped direction */
static void cInferTypeArguments(CInfer *c, TsType *source, TsType *target) {
  uint8_t *v = cVariances(target->target);
  int count = source->nargs < target->nargs ? source->nargs : target->nargs;
  for (int i = 0; i < count; i++) {
    uint8_t vi = v ? (i < target->target->ntypeParams ? v[i] : VF_Covariant) : VF_Covariant;
    if ((vi & 3) == VF_Contravariant) cInferContra(c, source->args[i], target->args[i]);
    else cInferFrom(c, source->args[i], target->args[i]);
  }
}

/* inferFromSignature: the parameters (contravariantly) and the return types of a non-generic source against the
 * erased target signature */
/* the element type of a rest parameter of array type (createArrayType's Array<T>): a readonly or other array is not ported */
static TsType *cRestElementOf(TsType *t, bool *ok) {
  if ((t->flags & TF_Object) && (t->objectFlags & OF_Reference) && t->target == cGlobalArrayDeclared() && t->nargs == 1) return t->args[0];
  *ok = false;
  return cErrorType;
}

/* getEffectiveRestType of a signature: the array type of its rest parameter (any gives any[]). A tuple rest type and an
 * unannotated rest parameter are not ported (*ok false). NULL without a rest parameter. */
static TsType *cSigEffectiveRest(CSig *sig, CParams *cp, bool *ok) {
  *ok = true;
  if (!cp->rest) return NULL;
  if (!cp->rest->type) { *ok = false; return NULL; }
  /* the rest type of an instance is its instantiated parameter symbol's type (cached on the symbol) */
  bool instRest = sig->mapper && sig->psyms && cp->n && cp->params[cp->n - 1] == cp->rest && sig->psyms[cp->n - 1];
  TsType *t = instRest ? cTypeOfSymbol(sig->psyms[cp->n - 1]) : cTypeFromNodeM(cp->rest->type, sig->mapper);
  if (t == cErrorType || cIsTupleType(t)) { *ok = false; return NULL; }
  if (t->flags & TF_Any) return cTypeReference(cGlobalArrayDeclared(), &cAnyType, 1);
  return t;
}

/* getRestTypeAtPosition(source, pos) for a source whose parameters have no tuple: the rest array itself, or Array<element> past
 * the rest position, or the empty tuple past the parameters. A tuple of the parameters from pos is not ported. */
static TsType *cSigRestAt(CSig *src, CParams *ps, TsType *srest, int pos, bool *ok) {
  int parameterCount = ps->n + (ps->rest ? 1 : 0);
  if (srest && pos >= parameterCount - 1) {
    if (pos == parameterCount - 1) return srest;
    TsType *el = cRestElementOf(srest, ok);
    if (!*ok) return cErrorType;
    return cTypeReference(cGlobalArrayDeclared(), &el, 1);
  }
  if (!srest && pos >= ps->n) return cTupleReference(NULL, 0, false);
  *ok = false;
  (void)src;
  return cErrorType;
}

/* applyToParameterTypes of inferFromSignature (strictFunctionTypes): the this types, then the parameters at each position
 * (contravariantly), then the rest parameter of the target against the rest of the source */
static bool cInferApplyToParams(CInfer *c, CSig *src, CParams *ps, CSig *tgt, CParams *pt) {
  bool okR;
  TsType *srest = cSigEffectiveRest(src, ps, &okR);
  if (!okR) return false;
  TsType *trest = cSigEffectiveRest(tgt, pt, &okR);
  if (!okR) return false;
  int targetNonRest = pt->n;
  int sourceCount = ps->n + (ps->rest ? 1 : 0);
  int paramCount = srest ? targetNonRest : (sourceCount < targetNonRest ? sourceCount : targetNonRest);
  if (ps->thisParam && pt->thisParam && ps->thisParam->type && pt->thisParam->type) {
    TsType *ts = cTypeFromNodeM(ps->thisParam->type, src->mapper), *tt = cTypeFromNodeM(pt->thisParam->type, tgt->mapper);
    if (ts == cErrorType || tt == cErrorType) return false;
    cInferContra(c, ts, tt);
  }
  for (int i = 0; i < paramCount; i++) {
    bool okS, okT;
    TsType *s = cSigParamType(src, ps, i, &okS);
    TsType *t = cSigParamType(tgt, pt, i, &okT);
    if (!okS || !okT || s == cErrorType || t == cErrorType) return false;
    cInferContra(c, s, t);
  }
  if (trest) {
    TsType *rt = cSigRestAt(src, ps, srest, paramCount, &okR);
    if (!okR || rt == cErrorType) return false;
    cInferContra(c, rt, trest);
  }
  return true;
}

/* getTypePredicateOfSignature (cached per signature and mapper): the type of the predicate of a signature's declaration */
static TsType *cPredicateTypeOf(CSig *sig) {
  for (int i = 0; i < cPredN; i++)
    if (cPredList[i].decl == sig->decl && cPredList[i].mapper == sig->mapper) return cPredList[i].val;
  TsType *v = cTypeFromNodeM(sig->decl->type->type, sig->mapper);
  if (cPredN == cPredCap) {
    cPredCap = cPredCap ? cPredCap * 2 : 16;
    cPredList = realloc(cPredList, sizeof(CPredEnt) * (size_t)cPredCap);
    if (!cPredList) abort();
  }
  cPredList[cPredN].decl = sig->decl; cPredList[cPredN].mapper = sig->mapper; cPredList[cPredN].val = v; cPredN++;
  return v;
}
/* the parameter index of a type predicate's identifier in its signature (-1 for this, -2 when it names no parameter) */
static int cPredicateParamIndex(TsNode *decl, TsNode *pred) {
  TsNode *pn = pred->parameterName;
  if (!pn || pn->kind != TSK_Identifier) return -1;
  if (!decl->parameters) return -2;
  for (int i = 0; i < decl->parameters->n; i++) {
    TsNode *p = decl->parameters->v[i];
    if (p->name && p->name->kind == TSK_Identifier && p->name->textLen == pn->textLen && memcmp(p->name->text, pn->text, (size_t)pn->textLen) == 0)
      return i;
  }
  return -2;
}
/* getBaseSignature (cached per signature and mapper): each type parameter is the instance of its constraint, instantiated
 * by the signature's mapper (unknown without one); a constraint that still refers to a type parameter is not ported */
static CBaseEnt *cBaseSignatureOf(CSig *sig) {
  for (int i = 0; i < cBaseN; i++)
    if (cBaseList[i].decl == sig->decl && cBaseList[i].mapper == sig->mapper) return &cBaseList[i];
  int n;
  TsType **tps = cSigTypeParams(sig->decl, &n);
  TsType **vals = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  bool fail = false;
  for (int k = 0; k < n && !fail; k++) {
    TsType *cn = cConstraintOf(tps[k]);
    if (cn && sig->mapper) cn = cInstantiate(cn, sig->mapper);
    if (cn && cCouldContain(cn)) fail = true;
    vals[k] = cn ? cn : cUnknownType;
  }
  if (cBaseN == cBaseCap) {
    cBaseCap = cBaseCap ? cBaseCap * 2 : 16;
    cBaseList = realloc(cBaseList, sizeof(CBaseEnt) * (size_t)cBaseCap);
    if (!cBaseList) abort();
  }
  CBaseEnt *e = &cBaseList[cBaseN++];
  e->decl = sig->decl; e->mapper = sig->mapper; e->vals = vals; e->n = n; e->fail = fail;
  return e;
}
static bool cInferSignature(CInfer *c, CSig *src0, CSig *tgt) {
  CSig *src = src0;
  if (cSigIsGeneric(src0)) {
    CBaseEnt *be = cBaseSignatureOf(src0);
    if (be->fail) return false;
    src = cGetSignatureInstance(src0, be->vals, be->n);
  }
  CSig tg = cErasedSignature(tgt);
  CParams ps, pt;
  if (!cSplitParams(src->decl, &ps) || !cSplitParams(tg.decl, &pt)) return false;
  /* inferFromSignature: the parameters of methods and constructors are bivariant */
  int tk = tg.decl->kind;
  bool biv = tk == TSK_MethodDeclaration || tk == TSK_MethodSignature || tk == TSK_Constructor;
  bool saveBiv = c->bivariant;
  c->bivariant = c->bivariant || biv;
  bool ok = cInferApplyToParams(c, src, &ps, &tg, &pt);
  c->bivariant = saveBiv;
  if (!ok) return false;
  /* applyToReturnTypes: a type predicate of the target infers from the type predicate of the source when both are
   * of the same kind (typePredicateKindsMatch: the same assertion and parameter); the return types otherwise */
  if (tg.decl->type && tg.decl->type->kind == TSK_TypePredicate) {
    TsNode *sp = src->decl->type, *tp = tg.decl->type;
    if (sp && sp->kind == TSK_TypePredicate && sp->type && tp->type && (sp->assertsModifier != NULL) == (tp->assertsModifier != NULL) &&
        cPredicateParamIndex(src->decl, sp) == cPredicateParamIndex(tg.decl, tp))
      cInferFrom(c, cPredicateTypeOf(src), cPredicateTypeOf(&tg));
    return true;
  }
  cInferFrom(c, cReturnTypeOf(src), cReturnTypeOf(&tg));
  return true;
}

static void cInferObjectsBody(CInfer *c, TsType *source, TsType *target);
/* isArrayOrTupleType: an Array or ReadonlyArray reference, or a tuple reference */
static bool cIsArrayOrTupleRef(TsType *t) {
  return (t->objectFlags & OF_Reference) && t->target &&
         (t->target == cGlobalArrayDeclared() || t->target == cGlobalReadonlyArrayDeclared() || (t->target->objectFlags & OF_Tuple));
}
/* inferFromIndexTypes of an array target: the number index of the source against the number index of the target */
static void cInferArrayIndexTypes(CInfer *c, TsType *source, TsType *target) {
  cResolveIndexInfos(source);
  cResolveIndexInfos(target);
  TsType *si = NULL, *ti = NULL;
  for (int i = 0; i < source->nindex; i++) if (source->index[i].keyType == cNumberType) si = source->index[i].type;
  for (int i = 0; i < target->nindex; i++) if (target->index[i].keyType == cNumberType) ti = target->index[i].type;
  if (si && ti) cInferFrom(c, si, ti);
}
/* inferFromObjectTypes: the properties of the target that the source has, then the signatures */
static void cInferObjects(CInfer *c, TsType *source, TsType *target) {
  /* invokeOnce: a pair already being inferred is not inferred again (node's circularity), and the depth is bounded */
  for (int i = 0; i < c->nvis; i++) if (c->visS[i] == source && c->visT[i] == target) return;
  if (c->nvis >= 100) { cUnported("inference of nested object types"); return; }
  if (c->nvis == c->capvis) {
    int cap = c->capvis ? 2 * c->capvis : 8;
    TsType **ns = ts_arena_alloc(sizeof(TsType *) * (size_t)cap), **nt = ts_arena_alloc(sizeof(TsType *) * (size_t)cap);
    for (int i = 0; i < c->nvis; i++) { ns[i] = c->visS[i]; nt[i] = c->visT[i]; }
    c->visS = ns; c->visT = nt; c->capvis = cap;
  }
  c->visS[c->nvis] = source; c->visT[c->nvis] = target; c->nvis++;
  cInferObjectsBody(c, source, target);
  c->nvis--;
}

static TsType *cTypeWithFacts(TsType *t, uint32_t facts);

/* the property name of a source property applies to an index key type (getLiteralTypeFromProperty and isApplicableIndexType):
 * a string key takes every name but a unique symbol one, a number key takes a numeric name. A name that may be numeric
 * but is not a plain integer is not ported (*unp) */
static bool cPropertyNameApplies(TsSymbol *p, TsType *keyType, bool *unp) {
  if (keyType != cStringType && keyType != cNumberType) { *unp = true; return false; }
  if (p->name.n >= 3 && memcmp(p->name.s, "__@", 3) == 0) return false;
  if (keyType == cStringType) return true;
  TsNode *d = p->valueDeclaration ? p->valueDeclaration : (p->ndecls ? p->decls[0] : NULL);
  if (d && d->name && d->name->kind == TSK_NumericLiteral) return true;
  int n = p->name.n;
  const char *s = p->name.s;
  if (n == 0) return false;
  bool digits = true;
  for (int i = 0; i < n; i++) if (s[i] < '0' || s[i] > '9') digits = false;
  if (digits && (n == 1 || s[0] != '0')) return true;
  if ((s[0] >= '0' && s[0] <= '9') || s[0] == '-' || s[0] == '.' || s[0] == 'I' || s[0] == 'N') { *unp = true; return false; }
  return false;
}

static void cInferObjectsBody(CInfer *c, TsType *source, TsType *target) {
  TsSymbol **tp;
  int nt = cPropertiesOf(target, &tp);
  for (int i = 0; i < nt; i++) {
    TsSymbol *sp = cPropertyOf(source, tp[i]->name);
    if (sp) cInferFrom(c, cTypeOfSymbol(sp), cTypeOfSymbol(tp[i]));
  }
  for (int kind = 0; kind < 2; kind++) {
    cResolveSignatures(source);
    cResolveSignatures(target);
    int ns = kind ? source->nconstruct : source->ncall, nn = kind ? target->nconstruct : target->ncall;
    CSig *ss = kind ? source->construct : source->call, *ts = kind ? target->construct : target->call;
    int len = ns < nn ? ns : nn;
    for (int i = 0; i < len; i++)
      if (!cInferSignature(c, &ss[ns - len + i], &ts[nn - len + i])) cUnported("inference from a generic or rest signature");
  }
  cResolveIndexInfos(target);
  if (!target->nindex) return;
  /* inferFromIndexTypes: the properties of an object literal or type literal source infer into the index signatures
   * of the target (not ported); otherwise each index signature of the target infers from the applicable index
   * signature of the source (getApplicableIndexInfo) */
  cResolveSignatures(source);
  /* getPriority2: a homomorphic mapped type infers into the index signatures of a mapped target with priority 8 */
  int prio2 = ((source->objectFlags & target->objectFlags) & OF_Mapped) ? 8 : 0;
  cResolveIndexInfos(source);
  /* isObjectTypeWithInferableIndex: an object literal, type literal, enum or namespace without signatures */
  bool inferable = source->symbol && !(source->objectFlags & OF_Tuple) && !(source->symbol->flags & SF_Class) &&
                   (source->symbol->flags & (SF_ObjectLiteral | SF_TypeLiteral | SF_RegularEnum | SF_ValueModule)) &&
                   source->ncall == 0 && source->nconstruct == 0;
  if (inferable) {
    TsSymbol **sp;
    int np = cPropertiesOf(source, &sp);
    for (int i = 0; i < target->nindex; i++) {
      CIndexInfo *ti = &target->index[i];
      TsType **pts = ts_arena_alloc(sizeof(TsType *) * (size_t)(np + source->nindex + 1));
      int npt = 0;
      for (int k = 0; k < np; k++) {
        bool unp = false;
        if (!cPropertyNameApplies(sp[k], ti->keyType, &unp)) { if (unp) { cUnported("inference into an index signature"); return; } continue; }
        TsType *pt = cTypeOfSymbol(sp[k]);
        /* removeMissingOrUndefinedType of an optional property (exactOptionalPropertyTypes is off) */
        if (sp[k]->flags & SF_Optional) pt = cTypeWithFacts(pt, 524288u /* TFC_NEUndefined */);
        if (pt == cErrorType) return;
        pts[npt++] = pt;
      }
      for (int k = 0; k < source->nindex; k++) {
        CIndexInfo *si = &source->index[k];
        if (si->keyType == ti->keyType || (ti->keyType == cStringType && si->keyType == cNumberType)) pts[npt++] = si->type;
      }
      if (npt) cInferWithPriority(c, cGetUnionType(pts, npt), ti->type, prio2);
    }
  }
  for (int i = 0; i < target->nindex; i++) {
    CIndexInfo *ti = &target->index[i];
    bool unp = false;
    CIndexInfo *si = cApplicableIndexInfo(source, ti->keyType, &unp);
    if (unp) { cUnported("inference into an index signature"); return; }
    if (si) cInferWithPriority(c, si->type, ti->type, prio2);
  }
}

static void cInferFrom(CInfer *c, TsType *source, TsType *target) {
  if (!cCouldContain(target)) return;
  if (source == target && (source->flags & (TF_Union | TF_Intersection))) {
    for (int i = 0; i < source->ntypes; i++) cInferFrom(c, source->types[i], source->types[i]);
    return;
  }
  if (target->flags & TF_Union) {
    TsType **srcs = (source->flags & TF_Union) ? source->types : &source;
    int ns = (source->flags & TF_Union) ? source->ntypes : 1;
    TsType **tgts = target->types;
    int nt = target->ntypes;
    /* the source list is copied: the matching works on its own lists */
    TsType **sc = ts_arena_alloc(sizeof(TsType *) * (size_t)(ns ? ns : 1));
    memcpy(sc, srcs, sizeof(TsType *) * (size_t)ns);
    TsType **tc = ts_arena_alloc(sizeof(TsType *) * (size_t)(nt ? nt : 1));
    memcpy(tc, tgts, sizeof(TsType *) * (size_t)nt);
    cInferMatching(c, &sc, &ns, &tc, &nt, false);
    cInferMatching(c, &sc, &ns, &tc, &nt, true);
    if (nt == 0) return;
    target = cGetUnionType(tc, nt);
    if (ns == 0) {
      cInferWithPriority(c, source, target, 1);
      return;
    }
    source = cGetUnionType(sc, ns);
  } else if (target->flags & TF_Intersection) {
    bool allObjects = true;
    for (int i = 0; i < target->ntypes; i++) allObjects = allObjects && (target->types[i]->flags & TF_Object);
    if (!allObjects && !(source->flags & TF_Union)) {
      TsType **sc = ts_arena_alloc(sizeof(TsType *) * (size_t)((source->flags & TF_Intersection) ? source->ntypes : 1));
      int ns = (source->flags & TF_Intersection) ? source->ntypes : 1;
      if (source->flags & TF_Intersection) memcpy(sc, source->types, sizeof(TsType *) * (size_t)ns);
      else sc[0] = source;
      TsType **tc = ts_arena_alloc(sizeof(TsType *) * (size_t)(target->ntypes ? target->ntypes : 1));
      memcpy(tc, target->types, sizeof(TsType *) * (size_t)target->ntypes);
      int nt = target->ntypes;
      cInferMatching(c, &sc, &ns, &tc, &nt, false);
      if (ns == 0 || nt == 0) return;
      source = cGetIntersectionType(sc, ns);
      target = cGetIntersectionType(tc, nt);
    }
  }
  if (target->flags & TF_TypeParameter) {
    int k = cInferIndexOf(c, target);
    if (k >= 0) cInferAddCandidate(c, k, source);
    return;
  }
  if ((target->flags & TF_Object) && (target->objectFlags & OF_Reference) && (source->flags & TF_Object) &&
      (source->objectFlags & OF_Reference) && source->target == target->target) {
    cInferTypeArguments(c, source, target);
    return;
  }
  if ((source->flags & TF_IndexedAccess) && (target->flags & TF_IndexedAccess)) {
    cInferFrom(c, source->iaObj, target->iaObj);
    cInferFrom(c, source->iaIdx, target->iaIdx);
    return;
  }
  if (target->flags & (TF_Union | TF_Intersection)) {
    cInferToMultiple(c, source, target);
    return;
  }
  if (source->flags & TF_Union) {
    for (int i = 0; i < source->ntypes; i++) cInferFrom(c, source->types[i], target);
    return;
  }
  if (target->flags & TF_IndexedAccess) return;   /* the simplified forms of T[K] are not ported; nothing to infer */
  if (target->flags & (TF_Index | TF_Conditional | TF_TemplateLiteral | TF_StringMapping)) {
    cUnported("inference into an index, conditional, template literal or string mapping type");
    return;
  }
  if (target->flags & TF_Object) {
    if (!(source->flags & TF_Object)) source = cApparentType(source);
    if (!source || !(source->flags & TF_Object)) return;
    /* inferFromObjectTypes: an array or tuple source against an Array or ReadonlyArray target infers from the index types */
    if ((target->objectFlags & OF_Reference) && target->target && (target->target == cGlobalArrayDeclared() || target->target == cGlobalReadonlyArrayDeclared()) &&
        cIsArrayOrTupleRef(source)) {
      cInferArrayIndexTypes(c, source, target);
      return;
    }
    cInferObjects(c, source, target);
  }
}

/* instantiateSignatureInContextOf without an inference context (compareSignaturesRelated): the source's type
 * parameters are inferred from the target's parameter types (priority 0) and return type (ReturnType priority),
 * with the candidates of the lowest priority kept; a covariant inference is the common supertype of the
 * candidates (subtype relation), a contravariant one their common subtype, and an unused parameter is unknown.
 * The constraint replaces a type that is not related to it. The subset: no literal, null or undefined
 * candidates, no this, rest or default type parameters, no constraint that mentions type parameters. Returns
 * false when the subset does not cover the signatures. */
static bool cSigInContextOf(CSig *src, CSig *tgt, CSig *out) {
  int n;
  TsType **tps = cSigTypeParams(src->decl, &n);
  CParams ps, pt;
  if (!cSplitParams(src->decl, &ps) || !cSplitParams(tgt->decl, &pt)) { (void)cUnported("too many parameters"); return false; }
  if (ps.thisParam || pt.thisParam || ps.rest || pt.rest) { (void)cUnported("signature inference with this or rest parameters"); return false; }
  int before = cUnportedCount;
  CInfer ci;
  cInferInit(&ci, tps, n);
  int np = ps.n < pt.n ? ps.n : pt.n;
  for (int i = 0; i < np; i++) {
    bool okS, okT;
    TsType *tt = cSigParamType(tgt, &pt, i, &okT);
    TsType *st = cSigParamType(src, &ps, i, &okS);
    if (!okS || !okT) continue;
    if (tt == cErrorType || st == cErrorType) return false;
    cInferFrom(&ci, tt, st);
  }
  TsType *tr = cReturnTypeOf(tgt), *sr = cReturnTypeOf(src);
  if (tr == cErrorType || sr == cErrorType) return false;
  ci.priority = 128;   /* InferencePriority.ReturnType */
  cInferFrom(&ci, tr, sr);
  ci.priority = 0;
  if (cUnportedCount != before) return false;
  TsType **args = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  for (int k = 0; k < n; k++) {
    TsType *inferred;
    if (ci.cn[k]) {
      TsType **cs = ci.cand[k];
      int nc = ci.cn[k];
      for (int i = 0; i < nc; i++) {
        if (cs[i]->flags & (TF_StringLiteral | TF_NumberLiteral | TF_BooleanLiteral | TF_BigIntLiteral | TF_Null | TF_Undefined)) {
          (void)cUnported("signature inference from a literal, null or undefined candidate");
          return false;
        }
      }
      TsType *s = cs[0];
      for (int i = 1; i < nc; i++) s = cIsRelatedWith(s, cs[i], REL_Subtype) ? cs[i] : s;   /* getCommonSupertype */
      inferred = s;
    } else if (ci.ctn[k]) {
      TsType **cs = ci.con[k];
      TsType *s = cs[0];
      for (int i = 1; i < ci.ctn[k]; i++) s = cIsRelatedWith(cs[i], s, REL_Subtype) ? cs[i] : s;   /* getCommonSubtype */
      inferred = s;
    } else {
      if (cTypeParamDefaultNode(tps[k])) { (void)cUnported("signature inference with a default type parameter"); return false; }
      inferred = cUnknownType;
    }
    TsType *c = cConstraintOf(tps[k]);
    if (c) {
      if (cCouldContain(c)) { (void)cUnported("signature inference with a constraint of type parameters"); return false; }
      if (cIsRelatedTo(inferred, c, IS_None) == RR_False) inferred = c;
    }
    args[k] = inferred;
  }
  out->decl = src->decl;
  out->isConstruct = src->isConstruct;
  out->thisType = src->thisType;
  TsMapper *infer = cCreateTypeMapper(tps, args, n);
  out->mapper = cCombineMappers(src->mapper, infer);
  cSigInstantiate(out, src, infer);   /* instantiateSignature of the source with the inference mapper */
  out->erased = true;
  return true;
}

/* compareSignaturesRelated with a generic source: the source is instantiated in the context of the target */
static bool cCompareSignaturesInContext(CSig *src, CSig *tgt) {
  if (src->decl == tgt->decl && src->mapper == tgt->mapper) return true;
  if (cSigIsGeneric(src)) {
    CSig inst;
    if (!cSigInContextOf(src, tgt, &inst)) return true;   /* not ported: the relation holds (conservative) */
    return cCompareSignaturesRelated(&inst, tgt);
  }
  return cCompareSignaturesRelated(src, tgt);
}

/* signaturesRelatedTo (68500s): every target signature needs a related source signature; with one signature on
 * each side the generic signatures are compared as they are, with several they are erased (eraseGenerics) */
static int cSignaturesRelated(TsType *s, TsType *t, bool isConstruct) {
  cResolveSignatures(s);
  cResolveSignatures(t);
  int ns = isConstruct ? s->nconstruct : s->ncall, nt = isConstruct ? t->nconstruct : t->ncall;
  CSig *ss = isConstruct ? s->construct : s->call, *ts = isConstruct ? t->construct : t->call;
  if (nt == 0) return RR_True;
  /* signaturesRelatedTo: several signatures are erased; the comparable relation erases a single pair too (71469) */
  bool erase = !(ns == 1 && nt == 1) || cCurRel == REL_Comparable;
  for (int j = 0; j < nt; j++) {
    bool hit = false;
    for (int i = 0; i < ns && !hit; i++) {
      if (!erase) { hit = cCompareSignaturesInContext(&ss[i], &ts[j]); continue; }
      CSig a = cErasedSignature(&ss[i]), b = cErasedSignature(&ts[j]);
      hit = cCompareSignaturesInContext(&a, &b);
    }
    if (!hit) return RR_False;
  }
  return RR_True;
}

/* indexSignaturesRelatedTo: each target index signature needs an applicable source one (or, for an object
 * literal or type literal source, each matching property) */
static int cIndexRelated(TsType *s, TsType *t) {
  cResolveIndexInfos(t);
  if (t->nindex == 0) return RR_True;
  cResolveIndexInfos(s);
  int result = RR_True;
  /* targetHasStringIndex: an index signature of type any accepts every non-primitive source (not in the strict subtype relation) */
  bool targetHasStringIndex = false;
  for (int j = 0; j < t->nindex; j++) if (t->index[j].keyType == cStringType) targetHasStringIndex = true;
  for (int j = 0; j < t->nindex; j++) {
    CIndexInfo *ti = &t->index[j];
    if (targetHasStringIndex && cCurRel != REL_StrictSubtype && (ti->type->flags & TF_Any)) continue;
    int found = -1;
    for (int i = 0; i < s->nindex && found < 0; i++)
      if (s->index[i].keyType == ti->keyType) found = i;
    if (found < 0 && ti->keyType == cNumberType) {
      for (int i = 0; i < s->nindex && found < 0; i++)
        if (s->index[i].keyType == cStringType) found = i;
    }
    int r;
    if (found >= 0) r = cIsRelatedTo(s->index[found].type, ti->type, IS_None);
    else if (s->symbol && (s->symbol->flags & (SF_TypeLiteral | SF_ObjectLiteral))) {
      /* membersRelatedToIndexer: every property of an object type with an inferable index */
      TsSymbol **props;
      int np = cPropertiesOf(s, &props);
      r = RR_True;
      for (int i = 0; i < np && r != RR_False; i++) {
        int rr = cIsRelatedTo(cTypeOfSymbol(props[i]), ti->type, IS_None);
        if (rr < r) r = rr;
      }
    } else r = RR_False;
    if (r == RR_False) return RR_False;
    if (r < result) result = r;
  }
  return result;
}

/* structuredTypeRelatedTo for object types: the variances of references of one target, else the members */
static int cStructuredObjects(TsType *s, TsType *t, int st) {
  if ((s->objectFlags & OF_Reference) && (t->objectFlags & OF_Reference) && s->target == t->target) {
    uint8_t *v = cVariances(t->target);
    if (!v) return RR_Maybe;   /* the variances are being computed (emptyArray in TypeScript) */
    return cRelateVariances(s, t, v);
  }
  int r = cPropertiesRelated(s, t, st);
  if (r == RR_False) return RR_False;
  int r2 = cSignaturesRelated(s, t, false);
  if (r2 == RR_False) return RR_False;
  if (r2 < r) r = r2;
  r2 = cSignaturesRelated(s, t, true);
  if (r2 == RR_False) return RR_False;
  if (r2 < r) r = r2;
  r2 = cIndexRelated(s, t);
  if (r2 == RR_False) return RR_False;
  return r2 < r ? r2 : r;
}

/* unionOrIntersectionRelatedTo / structuredTypeRelatedToWorker (the decomposition part) */
static int cTypeRelatedToSome(TsType *s, TsType *t, int st) {
  for (int i = 0; i < t->ntypes; i++) if (t->types[i] == s) return RR_True;   /* containsType */
  /* typeRelatedToSomeType: a string, boolean or bigint literal (and a number literal for the subtype relations)
   * is looked up in a primitive union instead of comparing each member */
  if ((t->objectFlags & OF_PrimitiveUnion) && cCurRel != REL_Comparable && !(s->flags & TF_EnumLiteral) &&
      ((s->flags & (TF_StringLiteral | TF_BooleanLiteral | TF_BigIntLiteral)) ||
       ((cCurRel == REL_Subtype || cCurRel == REL_StrictSubtype) && (s->flags & TF_NumberLiteral)))) {
    TsType *prim = (s->flags & TF_StringLiteral) ? cStringType : (s->flags & TF_NumberLiteral) ? cNumberType
                 : (s->flags & TF_BigIntLiteral) ? cBigIntType : NULL;
    bool found = false;
    for (int i = 0; prim && i < t->ntypes && !found; i++) found = t->types[i] == prim;
    return found ? RR_True : RR_False;
  }
  int result = RR_False;
  for (int i = 0; i < t->ntypes; i++) {
    int r = cIsRelatedTo(s, t->types[i], st);
    if (r == RR_True) return RR_True;
    if (r > result) result = r;
  }
  return result;
}

static int cTypeRelatedToEach(TsType *s, TsType *t, int st) {
  int result = RR_True;
  for (int i = 0; i < t->ntypes; i++) {
    int r = cIsRelatedTo(s, t->types[i], st);
    if (r == RR_False) return RR_False;
    if (r < result) result = r;
  }
  return result;
}

static int cEachTypeRelatedToType(TsType *s, TsType *t, int st) {
  int result = RR_True;
  for (int i = 0; i < s->ntypes; i++) {
    int r = cIsRelatedTo(s->types[i], t, st);
    if (r == RR_False) return RR_False;
    if (r < result) result = r;
  }
  return result;
}

static int cSomeTypeRelatedToType(TsType *s, TsType *t, int st) {
  int result = RR_False;
  for (int i = 0; i < s->ntypes; i++) {
    int r = cIsRelatedTo(s->types[i], t, st);
    if (r == RR_True) return RR_True;
    if (r > result) result = r;
  }
  return result;
}

/* ---- index and indexed access types (structuredTypeRelatedToWorker 70759-70971) ---- */
/* getBaseConstraintOfType: the base of a type parameter chain, or the type itself when it is not instantiable;
 * NULL when a type parameter has no constraint, cErrorType (unported) for the compound forms that are not ported */
static TsType *cBaseConstraintOf(TsType *t) {
  for (int depth = 0; t->flags & TF_TypeParameter; depth++) {
    if (t->isThisType || depth > 50) return cUnported("base constraint of a this type");
    TsType *c = cConstraintOf(t);
    if (!c) return NULL;
    t = c;
  }
  if (t->flags & (TF_IndexedAccess | TF_Index | TF_Conditional | TF_TemplateLiteral | TF_StringMapping))
    return cUnported("base constraint of an instantiable type");
  if (t->flags & (TF_Union | TF_Intersection)) {
    for (int i = 0; i < t->ntypes; i++)
      if (t->types[i]->flags & (TF_TypeParameter | TF_IndexedAccess | TF_Index | TF_Conditional | TF_TemplateLiteral | TF_StringMapping))
        return cUnported("base constraint of a generic union or intersection");
  }
  return t;
}

/* the value of o[key] for a relation: the property, or the applicable index signature unless noIdx (NoIndexSignatures);
 * NULL when there is none (an undefined result of getIndexedAccessTypeOrUndefined) */
static TsType *cIndexedPropertyOrNull(TsType *o, TsType *key, bool noIdx) {
  if (o->flags & (TF_Any | TF_Never)) {
    if (key->flags & (TF_StringLiteral | TF_NumberLiteral | TF_String | TF_Number)) return o;
    return cUnported("indexed access relation of any with another key");
  }
  if (o->flags & TF_Unknown) return NULL;
  if (!(o->flags & TF_Object)) return cUnported("indexed access relation on a primitive object");
  if (noIdx) {
    if (!(key->flags & (TF_StringLiteral | TF_NumberLiteral))) return NULL;
    BName name = (key->flags & TF_StringLiteral) ? cLiteralKey(key) : bnEscapeText(key->text, key->textLen);
    TsSymbol *p = cPropertyOf(o, name);
    return p ? cTypeOfSymbol(p) : NULL;
  }
  return cPropertyTypeForIndex(o, key);
}

/* getIndexedAccessTypeOrUndefined for a relation: a generic access is deferred as usual; a union key gives the
 * intersection of its members when writing and the union otherwise; a missing member gives NULL */
static TsType *cIndexedAccessForRelation(TsType *o, TsType *ix, bool writing, bool noIdx) {
  if (o == cErrorType || ix == cErrorType) return cErrorType;
  if (cIsGenericIndex(ix) || cIsGenericObject(o)) return cGetIndexedAccessType(o, ix, false);
  if (ix->flags & TF_Union && !(ix->flags & TF_Boolean)) {
    TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(ix->ntypes ? ix->ntypes : 1));
    for (int i = 0; i < ix->ntypes; i++) {
      parts[i] = cIndexedPropertyOrNull(o, ix->types[i], noIdx);
      if (!parts[i]) return NULL;
      if (parts[i] == cErrorType) return cErrorType;
    }
    return writing ? cGetIntersectionType(parts, ix->ntypes) : cGetUnionType(parts, ix->ntypes);
  }
  return cIndexedPropertyOrNull(o, ix, noIdx);
}

/* getConstraintOfIndexedAccess (getConstraintFromIndexedAccess): the access through the constraint of the index,
 * else of the object. NULL when there is none; cErrorType for a form whose simplification is not ported */
static TsType *cConstraintOfIndexed(TsType *ia) {
  TsType *ob = ia->iaObj, *ix = ia->iaIdx;
  if ((ob->flags & (TF_Union | TF_Intersection | TF_IndexedAccess | TF_Index | TF_Conditional)) ||
      (ix->flags & (TF_IndexedAccess | TF_Index | TF_Conditional)))
    return cUnported("constraint of an indexed access with a compound object or index");
  if (ix->flags & TF_TypeParameter) {
    TsType *c = cConstraintOf(ix);
    if (c) {
      TsType *r = cIndexedAccessForRelation(ob, c, false, false);
      if (r) return r;
    }
  }
  if (ob->flags & TF_TypeParameter) {
    TsType *c = cConstraintOf(ob);
    return c ? cIndexedAccessForRelation(c, ix, false, false) : NULL;
  }
  return NULL;
}

/* the target side of structuredTypeRelatedToWorker: keyof T and T[K] as targets. RR_False means the rule did not
 * relate them and the source side is tried next, as in TypeScript. */
static int cIndexTargetRelated(TsType *s, TsType *t, int st) {
  (void)st;
  if (t->flags & TF_Index) {
    TsType *tt = t->ixOf;
    if (s->flags & TF_Index) {
      int r = cIsRelatedTo(tt, s->ixOf, IS_None);
      if (r != RR_False) return r;
    }
    if (tt->objectFlags & OF_Tuple) { cUnported("keyof of a tuple"); return RR_True; }
    TsType *c = NULL;
    if (tt->flags & TF_TypeParameter) c = cConstraintOf(tt);
    else if (tt->flags & TF_IndexedAccess) {
      c = cConstraintOfIndexed(tt);
      if (c == cErrorType) return RR_True;
    }
    if (c) {
      if (cIsRelatedTo(s, cGetIndexType(c), IS_Target) == RR_True) return RR_True;
    } else if ((tt->flags & TF_Object) && (tt->objectFlags & OF_Mapped) && cIsGenericIndex(tt->mapConstraint)) {
      if (tt->typeNode && tt->typeNode->nameType) { (void)cUnported("keys of a mapped type with a name type"); return RR_True; }
      if (cIsRelatedTo(s, tt->mapConstraint, IS_None) == RR_True) return RR_True;
    }
    return RR_False;
  }
  /* t is T[K] */
  if (s->flags & TF_IndexedAccess) {
    int ro = cIsRelatedTo(s->iaObj, t->iaObj, IS_None);
    if (ro != RR_False) {
      int ri = cIsRelatedTo(s->iaIdx, t->iaIdx, IS_None);
      if (ri != RR_False) return (ro == RR_True && ri == RR_True) ? RR_True : RR_Maybe;
    }
  }
  if (cCurRel == REL_Assignable || cCurRel == REL_Comparable) {
    /* a target T[K] with a non-generic base is related through the access of its base constraints, written */
    TsType *bo = cBaseConstraintOf(t->iaObj), *bi = cBaseConstraintOf(t->iaIdx);
    if (bo == cErrorType || bi == cErrorType) return RR_True;
    if (!bo) bo = t->iaObj;
    if (!bi) bi = t->iaIdx;
    if (!cIsGenericObject(bo) && !cIsGenericIndex(bi)) {
      TsType *c = cIndexedAccessForRelation(bo, bi, true, bo != t->iaObj);
      if (c == cErrorType) return RR_True;
      if (c) {
        int r = cIsRelatedTo(s, c, IS_Target);
        if (r != RR_False) return r;
      }
    }
  }
  return RR_False;
}

/* the source side for keyof T and T[K] (the TypeVariable and Index branches): keyof T is string | number | symbol,
 * and T[K] through its constraint */
static int cIndexSourceRelated(TsType *s, TsType *t, int st) {
  if (s->flags & TF_Index) {
    TsType *keys = cGetUnionType((TsType *[]){cStringType, cNumberType, cESSymbolType}, 3);
    return cIsRelatedTo(keys, t, IS_Source);
  }
  if (t->flags & TF_IndexedAccess) return RR_False;   /* both T[K]: the constraint of the source is not related to T[K] */
  TsType *c = cConstraintOfIndexed(s);
  if (c == cErrorType) return RR_True;
  int r = cIsRelatedTo(c ? c : cUnknownType, t, st);
  if (r != RR_False) return r;
  if (s->iaObj->flags & TF_Object && (s->iaObj->objectFlags & OF_Mapped)) { (void)cUnported("indexed access of a mapped type as a source"); return RR_True; }
  return RR_False;
}

/* someType(t, flags & TypeParameter) */
static bool cHasTypeParamMember(TsType *t) {
  if (t->flags & TF_TypeParameter) return true;
  if (t->flags & TF_Union) for (int i = 0; i < t->ntypes; i++) if (t->types[i]->flags & TF_TypeParameter) return true;
  return false;
}

/* isReadonlyArrayType or a readonly tuple reference, and a mutable tuple target (structuredTypeRelatedToWorker: a tuple target
 * that is not readonly takes no readonly array or tuple) */
static bool cIsReadonlyArrayOrTuple(TsType *s) {
  return (s->objectFlags & OF_Reference) && s->target &&
         (s->target == cGlobalReadonlyArrayDeclared() || (s->target->objectFlags & OF_ReadonlyTuple));
}
static bool cIsMutableTupleTarget(TsType *t) {
  return (t->objectFlags & OF_Reference) && t->target && (t->target->objectFlags & OF_Tuple) && !(t->target->objectFlags & OF_ReadonlyTuple);
}

static int cStructuredRelated(TsType *s, TsType *t, int st) {
  if (s->flags & TF_Union) return cCurRel == REL_Comparable ? cSomeTypeRelatedToType(s, t, st) : cEachTypeRelatedToType(s, t, st);
  if (t->flags & TF_Union) {
    /* a type parameter source that is related to no member of the target union is related through its constraint
     * (structuredTypeRelatedToWorker falls through to the constraint of an instantiable source) */
    int r = cTypeRelatedToSome(s, t, st);
    if (r != RR_False || !(s->flags & (TF_TypeParameter | TF_IndexedAccess | TF_Index))) return r;
  }
  if (t->flags & TF_Intersection) return cTypeRelatedToEach(s, t, IS_Target);
  if (s->flags & TF_Intersection) {
    int r = cSomeTypeRelatedToType(s, t, IS_Source);
    if (r != RR_False) return r;
    if (t->flags & TF_Object) {
      /* the combined members of the intersection are not ported */
      cUnported("intersection source with structural target");
      return RR_True;
    }
    return RR_False;
  }
  if (t->flags & (TF_Index | TF_IndexedAccess)) {
    int r = cIndexTargetRelated(s, t, st);
    if (r != RR_False) return r;
  }
  if (s->flags & (TF_IndexedAccess | TF_Index)) return cIndexSourceRelated(s, t, st);
  /* comparable (structuredTypeRelatedToWorker, 70587): a type parameter source is related to a type parameter target
   * through the constraints of the source, each one a type parameter while it has one */
  if (cCurRel == REL_Comparable && (s->flags & TF_TypeParameter) && (t->flags & TF_TypeParameter)) {
    TsType *c = cConstraintOf(s);
    while (c && cHasTypeParamMember(c)) {
      int r = cIsRelatedTo(c, t, IS_Source);
      if (r != RR_False) return r;
      c = (c->flags & TF_TypeParameter) ? cConstraintOf(c) : NULL;   /* getConstraintOfTypeParameter */
    }
    return RR_False;
  }
  if (s->flags & TF_TypeParameter) {
    TsType *c = cConstraintOf(s);
    if (!c) return RR_False;
    return cIsRelatedTo(c, t, st);
  }
  if (t->flags & TF_TypeParameter) return RR_False;
  if (s->flags & TF_Object) {
    if (!(t->flags & TF_Object)) return RR_False;
    if (cIsMutableTupleTarget(t) && cIsReadonlyArrayOrTuple(s)) return RR_False;
    return cStructuredObjects(s, t, st);
  }
  /* a primitive source is compared through its apparent type */
  TsType *app = cApparentType(s);
  if (!app) return RR_False;
  if (!(t->flags & TF_Object)) return RR_False;
  return cStructuredObjects(app, t, st);
}

/* recursiveTypeRelatedTo (66480s): the relation cache, the maybe stack, the depth limits and the expanding check */
static bool cIsDeeplyNested(TsType *t, TsType **stack, int depth) {
  if (depth < 3) return false;
  TsType *id = (t->objectFlags & OF_Reference) ? t->target : (t->symbol ? (TsType *)(void *)t->symbol : t);
  int count = 0;
  for (int i = 0; i < depth; i++) {
    TsType *x = stack[i];
    TsType *xid = (x->objectFlags & OF_Reference) ? x->target : (x->symbol ? (TsType *)(void *)x->symbol : x);
    if (xid == id && ++count >= 3) return true;
  }
  return false;
}

static int cRecursiveRelated(TsType *s, TsType *t, int st) {
  CRelTab *tab = &cRelTab[cCurRel];
  int ci = cRelFind(tab, s, t, st);
  if (ci >= 0) return tab->e[ci].ok ? RR_True : RR_False;
  for (int i = 0; i < cMaybeN; i++)
    if (cMaybeKeys[i].s == s && cMaybeKeys[i].t == t && cMaybeKeys[i].st == st) return RR_Maybe;
  if (cSrcDepth >= 100 || cTgtDepth >= 100) { cRelOverflows++; return RR_False; }
  if (cIsDeeplyNested(s, cSrcStack, cSrcDepth) || cIsDeeplyNested(t, cTgtStack, cTgtDepth)) return RR_Maybe;
  if (cMaybeN == 256) { cRelOverflows++; return RR_False; }
  cMaybeKeys[cMaybeN].s = s;
  cMaybeKeys[cMaybeN].t = t;
  cMaybeKeys[cMaybeN].st = (uint8_t)st;
  cMaybeN++;
  cSrcStack[cSrcDepth++] = s;
  cTgtStack[cTgtDepth++] = t;
  int result = cStructuredRelated(s, t, st);
  cSrcDepth--;
  cTgtDepth--;
  cMaybeN--;
  if (result == RR_True || (result == RR_Maybe && cSrcDepth == 0 && cTgtDepth == 0)) {
    cRelStore(tab, s, t, st, true);
  } else if (result == RR_False) {
    cRelStore(tab, s, t, st, false);
  }
  return result;
}

/* isRelatedTo (isRelatedTo, 66900s): the simple rules, the weak type check, then the recursive comparison.
 * An unported type is related (and sets cUnportedHit), so the caller never reports a false error. */
/* getReducedType (node 63774) for the kinds of this port: an intersection with a property that is a discriminant of
 * type never, or a conflicting private property, is never; a union with intersection members reduces its members */
static TsType *cReducedType(TsType *t);
static bool cIsNeverReducedProperty(TsSymbol *p) {
  bool disc = !(p->flags & SF_Optional) && (p->checkFlags & (CK_HasNonUniformType | CK_HasLiteralType | CK_HasNeverType)) ==
              (CK_HasNonUniformType | CK_HasLiteralType) && (cTypeOfSymbol(p)->flags & TF_Never);
  return disc || (!p->valueDeclaration && (p->checkFlags & CK_ContainsPrivate));
}
static TsType *cReducedType(TsType *t) {
  if (t->flags & TF_Union) {
    if (!(t->objectFlags & OF_ContainsIntersections)) return t;
    if (t->resolvedReducedType) return t->resolvedReducedType;
    TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)(t->ntypes ? t->ntypes : 1));
    bool changed = false;
    for (int i = 0; i < t->ntypes; i++) {
      v[i] = cReducedType(t->types[i]);
      if (v[i] != t->types[i]) changed = true;
    }
    if (!changed) { t->resolvedReducedType = t; return t; }
    TsType *red = cGetUnionType(v, t->ntypes);
    if (red->flags & TF_Union) red->resolvedReducedType = red;
    t->resolvedReducedType = red;
    return red;
  }
  if (t->flags & TF_Intersection) {
    if (!(t->objectFlags & OF_NeverIntComputed)) {
      t->objectFlags |= OF_NeverIntComputed;
      TsSymbol **props;
      int n = cPropertiesOfUnionOrIntersection(t, &props);
      for (int i = 0; i < n; i++)
        if (cIsNeverReducedProperty(props[i])) { t->objectFlags |= OF_NeverIntersection; break; }
    }
    return (t->objectFlags & OF_NeverIntersection) ? cNeverType : t;
  }
  return t;
}

/* getNormalizedType for the kinds of this port (isRelatedTo normalizes both sides; the fresh literal and simplified
 * forms are not ported: the relation takes the regular literal and the simplified type as they are) */
static TsType *cNormalizeType(TsType *t) {
  for (;;) {
    TsType *r = (t->flags & (TF_Union | TF_Intersection)) ? cReducedType(t) : t;
    if (r == t) return t;
    t = r;
  }
}

static int cIsRelatedTo(TsType *s, TsType *t, int st) {
  /* getNormalizedType: a fresh literal relates as its regular type; unions and intersections are reduced */
  s = cNormalizeType(cRegularType(s));
  t = cNormalizeType(cRegularType(t));
  if (s == t) return RR_True;
  if (s == cErrorType || t == cErrorType) { cUnportedHit = true; return RR_True; }
  if ((s->flags | t->flags) & (TF_Conditional | TF_TemplateLiteral | TF_StringMapping)) { cUnported("relation of an indexed access, index, conditional, template literal or string mapping type"); return RR_True; }
  /* the comparable relation also tries the simple rules with the target as the source (isRelatedTo, 69257) */
  if ((cCurRel == REL_Comparable && !(t->flags & TF_Never) && cIsSimpleRelated(t, s, cCurRel)) || cIsSimpleRelated(s, t, cCurRel))
    return RR_True;
  /* isRelatedTo (5.9.2): a type parameter is related to its own constraint without a cache entry */
  if ((s->flags & TF_TypeParameter) && cConstraintOf(s) == t) return RR_True;
  if (!cIsStructured(s) && !cIsStructured(t)) return RR_False;
  /* isPerformingCommonPropertyChecks: a weak target needs a common property (comparable: only from a unit source) */
  if (cCurRel != REL_Identity && (cCurRel != REL_Comparable || cIsUnitType(s)) && !(st & IS_Target) && (t->flags & (TF_Object | TF_Intersection)) && cIsWeakType(t)) {
    /* getPropertiesOfType of a primitive source is that of its apparent type */
    TsType *sa = (s->flags & TF_Object) ? s : cApparentType(s);
    if (sa) {
      TsSymbol **sp;
      bool srcHasProps = cPropertiesOf(sa, &sp) > 0;
      if (!srcHasProps) { cResolveSignatures(sa); srcHasProps = sa->ncall || sa->nconstruct; }
      if (srcHasProps && !cHasCommonProperties(sa, t)) return RR_False;
    }
  }
  cRelDepth++;
  /* skipCaching (isRelatedTo): a union source of fewer than four members against a non-union target, or a union
   * target of fewer than four members against a non-structured source, is related without a cache entry */
  bool skipCaching = ((s->flags & TF_Union) && s->ntypes < 4 && !(t->flags & TF_Union)) ||
                     ((t->flags & TF_Union) && t->ntypes < 4 && !cIsStructured(s));
  int r = skipCaching ? cStructuredRelated(s, t, st) : cRecursiveRelated(s, t, st);
  cRelDepth--;
  return r;
}

/* ---- the entry points ---- */
/* anyFunctionType as the source: its properties are none, so the target's required properties decide; its signatures
 * are related to any target (signaturesRelatedTo). A target with index signatures is not ported. */
static bool cAnyFunctionRelated(TsType *t) {
  if (t == cAnyFunctionType || (t->flags & (TF_Any | TF_Unknown))) return true;
  if (t->flags & TF_Union) {
    for (int i = 0; i < t->ntypes; i++) if (cAnyFunctionRelated(t->types[i])) return true;
    return false;
  }
  if (!(t->flags & TF_Object)) return false;
  cResolveIndexInfos(t);
  if (t->nindex) { cUnported("anyFunctionType against an index signature"); return true; }
  TsSymbol **props;
  int n = cPropertiesOf(t, &props);
  for (int i = 0; i < n; i++) if (!(props[i]->flags & SF_Optional)) return false;
  return true;
}

/* isTypeRelatedTo (69): any relation, Maybe counts as related at the top */
static bool cIsRelatedWith(TsType *s, TsType *t, int rel) {
  /* isTypeRelatedTo: fresh literal types relate as their regular types */
  s = cRegularType(s);
  t = cRegularType(t);
  if (s == cAnyFunctionType) return cAnyFunctionRelated(t);
  int saved = cCurRel;
  cCurRel = rel;
  bool r;
  if (rel == REL_Identity) r = cIdentical(s, t);
  else if (s == t) r = true;
  else if ((rel == REL_Comparable && !(t->flags & TF_Never) && cIsSimpleRelated(t, s, rel)) || cIsSimpleRelated(s, t, rel)) r = true;
  else if ((s->flags | t->flags) & (TF_Conditional | TF_TemplateLiteral | TF_StringMapping)) { cUnported("relation of an indexed access, index, conditional, template literal or string mapping type"); r = true; }
  else if (!cIsStructured(s) && !cIsStructured(t)) r = false;
  else {
    int ci = cRelFind(&cRelTab[rel], s, t, IS_None);
    if (ci >= 0) r = cRelTab[rel].e[ci].ok;
    else r = cIsRelatedTo(s, t, IS_None) != RR_False;
  }
  cCurRel = saved;
  return r;
}

/* isTypeComparableTo and areTypesComparable (68409): the comparable relation in either direction */
static bool cAreTypesComparable(TsType *a, TsType *b) {
  return cIsRelatedWith(a, b, REL_Comparable) || cIsRelatedWith(b, a, REL_Comparable);
}

/* checkTypeRelatedTo: the relation, and a diagnostic with the code at the node when it fails */
static bool cCheckTypeRelatedTo(TsType *s, TsType *t, int rel, TsNode *errNode, int code) {
  bool r = cIsRelatedWith(s, t, rel);
  if (!r && errNode) cDiagAdd(cSourceFileOf(errNode), cNodeStart(errNode), code, NULL);
  return r;
}

static void cInitRelations(void) {
  cAnyFunctionType = cNewType(TF_Object);
  cAnyFunctionType->objectFlags = OF_Anonymous;
  cMarkerSuper = cNewType(TF_TypeParameter);
  cMarkerSuper->symbol = cCreateSymbol(SF_TypeParameter, bnC("markerSuperType"));
  cMarkerSub = cNewType(TF_TypeParameter);
  cMarkerSub->symbol = cCreateSymbol(SF_TypeParameter, bnC("markerSubType"));
  cMarkerOther = cNewType(TF_TypeParameter);
  cMarkerOther->symbol = cCreateSymbol(SF_TypeParameter, bnC("markerOtherType"));
  cMarkerSub->tpConstraint = cMarkerSuper;
  cMarkerSub->tpConstState = 2;
  memset(cGlobalStructTypes, 0, sizeof cGlobalStructTypes);
}

#endif
