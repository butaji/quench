/*
 * Checker port, instantiation (TypeScript 5.9.2 checker.ts; lib/typescript.js line numbers in
 * comments): type mappers (createTypeMapper, getMappedType, combineTypeMappers), instantiateType with
 * the active-mapper caches and the instantiation count (instantiateTypeWithAlias, 68173), instantiation
 * of type parameters, type references (createTypeReference, 64656, cached per target), anonymous object
 * types (getObjectTypeInstantiation, 67951; instantiateAnonymousType, 68125) with their outer type
 * parameters (getOuterTypeParameters, 61510; isTypeParameterPossiblyReferenced, 68008), unions and
 * intersections (getUnionType, 65992; getIntersectionType, 66276; cached by type list), and instantiated
 * symbols (instantiateSymbol, 67924; getTypeOfInstantiatedSymbol, 61381).
 *
 * NOT PORTED: this types (the outer type parameter list has no thisType), indexed access, conditional,
 * mapped, template literal, string mapping, index and substitution types, reverse mapped types, and
 * literal reduction of unions with a literal and its primitive (such a union sets cUnported). The
 * couldContainTypeVariables flag is computed on demand; its result is the same.
 */
#ifndef TS_INSTANTIATE_H
#define TS_INSTANTIATE_H

/* the mapper kinds (TypeMapKind, 67812-67880): simple, array, composite and merged */
enum { MK_Simple = 0, MK_Array = 1, MK_Composite = 4, MK_Merged = 5 };
typedef struct TsMapper TsMapper;
struct TsMapper { int kind; TsType **src, **tgt; int n; TsMapper *m1, *m2; };

static bool cIsThisless(TsSymbol *s);
static TsType *cDeclaredTypeOfClassOrInterface(TsSymbol *sym);
static TsType *cGetIndexedAccessType(TsType *o, TsType *ix, bool accessNode);
static TsType *cGetIndexType(TsType *t);
static TsType *cGetUnionType(TsType **in, int n);
static int cInstTotal;              /* totalInstantiationCount (getInstantiationCount) */
static int cInstDepth;              /* instantiationDepth */

static TsType **cCopyTypes(TsType **v, int n) {
  TsType **r = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  if (n && v) memcpy(r, v, sizeof(TsType *) * (size_t)n);
  return r;
}

static TsMapper *cNewMapper(int kind, TsType **src, TsType **tgt, int n) {
  TsMapper *m = ts_arena_zalloc(sizeof(TsMapper));
  m->kind = kind;
  m->src = src;
  m->tgt = tgt;
  m->n = n;
  return m;
}

/* createTypeMapper (67812): one source makes a simple mapper; a missing target is any */
static TsMapper *cCreateTypeMapper(TsType **src, TsType **tgt, int n) {
  if (n == 1) {
    TsType **s = cCopyTypes(src, 1), **t = cCopyTypes(NULL, 1);
    t[0] = tgt ? tgt[0] : cAnyType;
    return cNewMapper(MK_Simple, s, t, 1);
  }
  TsType **s = cCopyTypes(src, n), **t = ts_arena_alloc(sizeof(TsType *) * (size_t)(n ? n : 1));
  for (int i = 0; i < n; i++) t[i] = tgt ? tgt[i] : cAnyType;
  return cNewMapper(MK_Array, s, t, n);
}

/* combineTypeMappers (mapper1 ? composite : mapper2) and mergeTypeMappers */
static TsMapper *cCombineMappers(TsMapper *m1, TsMapper *m2) {
  if (!m1) return m2;
  TsMapper *m = cNewMapper(MK_Composite, NULL, NULL, 0);
  m->m1 = m1;
  m->m2 = m2;
  return m;
}

static TsType *cInstantiate(TsType *t, TsMapper *m);
static TsType *cGetConditionalTypeInstantiation(TsType *t, TsMapper *m);   /* ts-conditional.h */
static TsType **cInferTypeParamsOf(TsNode *n, int *count);
static TsType *cGetTemplateLiteralType(CStr *texts, TsType **types, int n);   /* ts-template.h */
static TsType *cGetStringMappingType(TsSymbol *sym, TsType *type);          /* ts-template.h */

/* getMappedType (67771) */
static TsType *cGetMappedType(TsType *t, TsMapper *m) {
  switch (m->kind) {
    case MK_Simple: return t == m->src[0] ? m->tgt[0] : t;
    case MK_Array:
      for (int i = 0; i < m->n; i++)
        if (t == m->src[i]) return m->tgt[i];
      return t;
    default: {
      TsType *t1 = cGetMappedType(t, m->m1);
      return t1 != t && m->kind == MK_Composite ? cInstantiate(t1, m->m2) : cGetMappedType(t1, m->m2);
    }
  }
}

/* couldContainTypeVariables (72810): cached on the type (0 not computed, 1 yes, 2 no) */
static void cResolveSignatures(TsType *t);   /* ts-types.h */
static void cResolveIndexInfos(TsType *t);
static int cPropertiesOf(TsType *t, TsSymbol ***out);
static bool cCouldContain(TsType *t) {
  if (t->couldContain) return t->couldContain == 1;
  bool r = false;
  if (t->flags & TF_TypeParameter) {
    r = true;
  } else if (t->flags & TF_Object) {
    if (t->objectFlags & OF_Reference) {
      for (int i = 0; i < t->nargs && !r; i++) r = cCouldContain(t->args[i]);
    } else if (t->objectFlags & OF_Anonymous) {
      if (t->typeNode && t->typeNode->kind == TSK_TypeLiteral && t->symbol && (t->symbol->flags & SF_TypeLiteral)) {
        /* a type literal mentions a type parameter through its signatures, properties or index signatures. Assumed
         * false while it is looked at, so a type literal that refers to itself is not walked again */
        t->couldContain = 2;
        cResolveSignatures(t);
        if (t->ncall || t->nconstruct) r = true;
        TsSymbol **props;
        int np = r ? 0 : cPropertiesOf(t, &props);
        for (int k = 0; k < np && !r; k++) r = cCouldContain(cTypeOfSymbol(props[k]));
        if (!r) {
          cResolveIndexInfos(t);
          for (int k = 0; k < t->nindex && !r; k++) r = cCouldContain(t->index[k].keyType) || cCouldContain(t->index[k].type);
        }
      } else {
        r = t->symbol && (t->symbol->flags & (SF_Function | SF_Method | SF_Class | SF_TypeLiteral | SF_ObjectLiteral)) &&
            t->symbol->ndecls > 0;
      }
    } else if (t->objectFlags & OF_Mapped) {
      r = true;   /* a mapped type may refer to the type parameters of its declaration */
    }
  } else if (t->flags & (TF_Union | TF_Intersection)) {
    for (int i = 0; i < t->ntypes && !r; i++) r = cCouldContain(t->types[i]);
  } else if (t->flags & (TF_IndexedAccess | TF_Index | TF_Conditional | TF_TemplateLiteral | TF_StringMapping)) {
    r = true;   /* instantiable types */
  }
  t->couldContain = r ? 1 : 2;
  return r;
}

/* ---- the instantiations of a generic type, by type arguments (target.instantiations) ---- */
static TsInst *cInstFind(TsType *target, TsType **key, int n) {
  for (int i = 0; i < target->ninsts; i++) {
    TsInst *e = &target->insts[i];
    if (e->n != n) continue;
    bool same = true;
    for (int k = 0; k < n && same; k++) same = e->key[k] == key[k];
    if (same) return e;
  }
  return NULL;
}

static void cInstAdd(TsType *target, TsType **key, int n, TsType *val) {
  if (target->ninsts == target->capInsts) {
    target->capInsts = target->capInsts ? target->capInsts * 2 : 4;
    TsInst *v = ts_arena_alloc(sizeof(TsInst) * (size_t)target->capInsts);
    if (target->ninsts) memcpy(v, target->insts, sizeof(TsInst) * (size_t)target->ninsts);
    target->insts = v;
  }
  TsInst *e = &target->insts[target->ninsts++];
  e->key = cCopyTypes(key, n);
  e->n = n;
  e->val = val;
}

/* createTypeReference (64656): a reference to a generic type with type arguments */
static TsType *cTypeReference(TsType *target, TsType **args, int nargs) {
  TsInst *e = cInstFind(target, args, nargs);
  if (e) return e->val;
  TsType *r = cNewType(TF_Object);
  r->objectFlags = OF_Reference;
  r->target = target;
  r->symbol = target->symbol;
  r->args = cCopyTypes(args, nargs);
  r->nargs = nargs;
  cInstAdd(target, args, nargs, r);
  return r;
}

/* ---- type nodes under a mapper ---- */
static TsType *cTypeFromNodeM(TsNode *n, TsMapper *m) {
  TsType *t = cTypeFromNode(n);
  return m ? cInstantiate(t, m) : t;
}

/* ---- outer type parameters (getOuterTypeParameters) ---- */
static BName cNameOfTP(TsType *tp) { return tp->symbol->name; }

/* the children of a node that can hold a reference to a type parameter (forEachChild) */
static bool cContainsTypeRef(TsNode *n, TsType *tp);

static bool cListHasRef(TsList *l, TsType *tp) {
  for (int i = 0; l && i < l->n; i++)
    if (cContainsTypeRef(l->v[i], tp)) return true;
  return false;
}

/* containsReference (68008): a type reference to the name, or a method whose signature mentions it */
static bool cContainsTypeRef(TsNode *n, TsType *tp) {
  if (!n) return false;
  switch (n->kind) {
    case TSK_ThisType: return tp->isThisType;
    case TSK_Identifier:
      /* a bare type reference to the name (maybeTypeParameterReference: not a generic reference's name) */
      return !tp->isThisType && n->parent && n->parent->kind == TSK_TypeReference && n->parent->typeName == n &&
             !n->parent->typeArguments && bnEq(bnEscapeText(n->text, n->textLen), cNameOfTP(tp));
    case TSK_TypeQuery: return true;
    case TSK_MethodDeclaration: case TSK_MethodSignature:
      return (!n->type && n->body) || cListHasRef(n->typeParameters, tp) || cListHasRef(n->parameters, tp) ||
             (n->type && cContainsTypeRef(n->type, tp));
    default: break;
  }
#define TS_CHILD(f) if (n->f && cContainsTypeRef(n->f, tp)) return true;
#define TS_LIST(f) if (cListHasRef(n->f, tp)) return true;
  TS_CHILD(name) TS_CHILD(expression) TS_CHILD(initializer) TS_CHILD(type) TS_CHILD(body) TS_CHILD(questionToken)
  TS_CHILD(exclamationToken) TS_CHILD(asteriskToken) TS_CHILD(dotDotDotToken) TS_CHILD(literal) TS_CHILD(left)
  TS_CHILD(right) TS_CHILD(operatorToken) TS_CHILD(condition) TS_CHILD(whenTrue) TS_CHILD(whenFalse)
  TS_CHILD(thenStatement) TS_CHILD(elseStatement) TS_CHILD(statement) TS_CHILD(tag) TS_CHILD(tagName)
  TS_CHILD(templateNode) TS_CHILD(qualifier) TS_CHILD(constraint) TS_CHILD(defaultType) TS_CHILD(typeName)
  TS_CHILD(argument) TS_CHILD(block) TS_CHILD(finallyBlock) TS_CHILD(catchClause) TS_CHILD(variableDeclaration)
  TS_CHILD(label) TS_CHILD(incrementor) TS_CHILD(declaration) TS_CHILD(moduleReference) TS_CHILD(moduleSpecifier)
  TS_CHILD(importClause) TS_CHILD(namedBindings) TS_CHILD(propertyName) TS_CHILD(checkType) TS_CHILD(extendsType)
  TS_CHILD(trueType) TS_CHILD(falseType) TS_CHILD(objectType) TS_CHILD(indexType) TS_CHILD(elementType)
  TS_CHILD(nameOrPath) TS_CHILD(endOfFileToken) TS_CHILD(head) TS_CHILD(exportClause) TS_CHILD(node1)
  TS_CHILD(node2) TS_CHILD(node3) TS_CHILD(node4) TS_CHILD(node5) TS_CHILD(typeExpression) TS_CHILD(readonlyToken)
  TS_CHILD(nameType) TS_CHILD(exprName) TS_CHILD(attributes_node) TS_CHILD(typeParameter) TS_CHILD(parameterName)
  TS_CHILD(assertsModifier) TS_CHILD(equalsGreaterThanToken) TS_CHILD(colonToken) TS_CHILD(questionDotToken)
  TS_CHILD(argumentExpression) TS_CHILD(operand) TS_CHILD(objectAssignmentInitializer) TS_CHILD(equalsToken)
  TS_CHILD(declarationList) TS_CHILD(caseBlock) TS_CHILD(awaitModifier)
  TS_LIST(modifiers) TS_LIST(typeParameters) TS_LIST(parameters) TS_LIST(members) TS_LIST(statements)
  TS_LIST(arguments) TS_LIST(elements) TS_LIST(properties) TS_LIST(heritageClauses) TS_LIST(types)
  TS_LIST(declarations) TS_LIST(typeArguments) TS_LIST(clauses) TS_LIST(spans) TS_LIST(jsDoc) TS_LIST(tokens)
  TS_LIST(qualifiers) TS_LIST(assertions) TS_LIST(attributes) TS_LIST(expressions) TS_LIST(list1) TS_LIST(list2)
#undef TS_CHILD
#undef TS_LIST
  return false;
}

/* isTypeParameterPossiblyReferenced (68008) for one declaration */
static bool cPossiblyReferenced(TsType *tp, TsNode *node) {
  TsSymbol *ts = tp->symbol;
  if (ts && ts->ndecls == 1) {
    TsNode *container = ts->decls[0]->parent;
    for (TsNode *x = node; x != container; x = x->parent)
      if (!x || x->kind == TSK_Block) return true;
    return cContainsTypeRef(node, tp);
  }
  return true;
}

/* getOuterTypeParameters (61510) without this types: the type parameters of the enclosing declarations,
 * outermost first, each declaration's own list appended without duplicates */
static TsType **cOuterTypeParamsOf(TsNode *node, bool includeThis, int *count) {
  for (TsNode *p = node->parent; p; p = p->parent) {
    switch (p->kind) {
      case TSK_ClassDeclaration: case TSK_ClassExpression: case TSK_InterfaceDeclaration: case TSK_CallSignature:
      case TSK_ConstructSignature: case TSK_MethodSignature: case TSK_FunctionType: case TSK_ConstructorType:
      case TSK_FunctionDeclaration: case TSK_MethodDeclaration: case TSK_FunctionExpression: case TSK_ArrowFunction:
      case TSK_TypeAliasDeclaration: {
        int n = 0;
        TsType **outer = cOuterTypeParamsOf(p, includeThis, &n);
        int own = p->typeParameters ? p->typeParameters->n : 0;
        TsType *thisTp = NULL;   /* getOuterTypeParameters: the this type of a class or interface (includeThisTypes) */
        if (includeThis && (p->kind == TSK_ClassDeclaration || p->kind == TSK_ClassExpression || p->kind == TSK_InterfaceDeclaration) && p->symbol)
          thisTp = cDeclaredTypeOfClassOrInterface(cGetMergedSymbol(p->symbol))->thisType;
        TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)(n + own + 2));
        if (n) memcpy(v, outer, sizeof(TsType *) * (size_t)n);
        int cnt = n;
        for (int i = 0; i < own; i++) {
          TsSymbol *ps = p->typeParameters->v[i]->symbol;
          if (!ps) { cUnported("type parameter without symbol"); continue; }
          TsType *tp = cTypeOfTypeParameter(cGetMergedSymbol(ps));
          bool seen = false;
          for (int k = 0; k < cnt && !seen; k++) seen = v[k] == tp;
          if (!seen) v[cnt++] = tp;
        }
        if (thisTp) v[cnt++] = thisTp;
        *count = cnt;
        return v;
      }
      case TSK_MappedType: {
        /* getOuterTypeParameters: the outer ones, then the type parameter of the mapped type */
        int n = 0;
        TsType **outer = cOuterTypeParamsOf(p, includeThis, &n);
        TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)(n + 1));
        if (n) memcpy(v, outer, sizeof(TsType *) * (size_t)n);
        v[n] = cTypeOfTypeParameter(cGetMergedSymbol(p->typeParameter->symbol));
        *count = n + 1;
        return v;
      }
      case TSK_ConditionalType: {
        /* concatenate (outer, infer type parameters) */
        int n = 0, ni = 0;
        TsType **outer = cOuterTypeParamsOf(p, includeThis, &n);
        TsType **ip = cInferTypeParamsOf(p, &ni);
        *count = n + ni;
        if (!*count) return NULL;
        TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)*count);
        if (n) memcpy(v, outer, sizeof(TsType *) * (size_t)n);
        if (ni) memcpy(v + n, ip, sizeof(TsType *) * (size_t)ni);
        return v;
      }
      default: break;
    }
  }
  *count = 0;
  return NULL;
}

/* the outer type parameters of an anonymous type's declaration, filtered for methods and type literals */
static void cEnsureOuterTypeParams(TsType *target) {
  if (target->outerDone) return;
  target->outerDone = true;
  TsSymbol *s = target->symbol;
  if (!s || !s->ndecls) { cUnported("anonymous type without declaration"); return; }
  int n = 0;
  TsType **tps = cOuterTypeParamsOf(s->decls[0], true, &n);
  if (n && (s->flags & (SF_Method | SF_TypeLiteral))) {
    int k = 0;
    for (int i = 0; i < n; i++) {
      bool any = false;
      for (int d = 0; d < s->ndecls && !any; d++) any = cPossiblyReferenced(tps[i], s->decls[d]);
      if (any) tps[k++] = tps[i];
    }
    n = k;
  }
  target->outerTps = tps;
  target->nOuterTps = n;
}

/* instantiateAnonymousType (68125): a new anonymous type of the target with the mapper */
static TsType *cInstantiateAnonymousType(TsType *target, TsMapper *m) {
  TsType *r = cNewType(TF_Object);
  r->objectFlags = target->objectFlags | OF_Instantiated;
  r->symbol = target->symbol;
  r->target = target;
  r->mapper = m;
  r->typeNode = target->typeNode;      /* mapped types: their declaration */
  r->mapTp = target->mapTp;
  r->mapConstraint = target->mapConstraint;
  return r;
}

/* instantiateConstituent (instantiateMappedType): a constituent of the homomorphic type variable is instantiated
 * with the variable bound to it */
static TsType *cInstantiateConstituent(TsType *target, TsType *tv, TsType *t, TsMapper *m) {
  if (!(t->flags & (TF_Any | TF_Unknown | TF_Object | TF_Intersection | TF_TypeParameter | TF_IndexedAccess | TF_Conditional)))
    return t;
  if (t->flags & TF_Any) return cUnported("homomorphic mapped type over any");
  if (t->flags & TF_Object && (t->objectFlags & OF_Reference)) return cUnported("homomorphic mapped type over a reference");
  TsMapper *unary = cNewMapper(MK_Simple, cCopyTypes(&tv, 1), cCopyTypes(&t, 1), 1);
  TsMapper *pm = m ? cNewMapper(MK_Merged, NULL, NULL, 0) : unary;
  if (m) { pm->m1 = unary; pm->m2 = m; }
  return cInstantiateAnonymousType(target, pm);
}

/* instantiateMappedType: a homomorphic mapped type (keyof T with a type parameter T) over an instantiated T that
 * changed distributes over its union constituents; other mapped types are anonymous instantiations */
static TsType *cInstantiateMappedType(TsType *target, TsMapper *m) {
  TsType *tv = target->mapConstraint && (target->mapConstraint->flags & TF_Index) && target->mapConstraint->ixOf &&
               (target->mapConstraint->ixOf->flags & TF_TypeParameter) ? target->mapConstraint->ixOf : NULL;
  if (tv) {
    TsType *mv = cInstantiate(tv, m);
    if (mv != tv) {
      if (mv->flags & TF_Union) {
        TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(mv->ntypes ? mv->ntypes : 1));
        for (int i = 0; i < mv->ntypes; i++) parts[i] = cInstantiateConstituent(target, tv, mv->types[i], m);
        return cGetUnionType(parts, mv->ntypes);
      }
      return cInstantiateConstituent(target, tv, mv, m);
    }
  }
  return cInstantiateAnonymousType(target, m);
}

/* getObjectTypeInstantiation (67951) for anonymous object types */
static TsType *cObjectTypeInstantiation(TsType *t, TsMapper *m) {
  TsType *target = (t->objectFlags & OF_Instantiated) ? t->target : t;
  cEnsureOuterTypeParams(target);
  int n = target->nOuterTps;
  if (!n) return t;
  if (!target->instsInit) {
    /* the target is its own instantiation for its own type parameters */
    target->instsInit = true;
    cInstAdd(target, target->outerTps, n, target);
  }
  TsMapper *combined = cCombineMappers(t->mapper, m);
  TsType **targs = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
  bool couldContain = false;
  for (int i = 0; i < n; i++) {
    targs[i] = cGetMappedType(target->outerTps[i], combined);
    if (cCouldContain(targs[i])) couldContain = true;
  }
  TsInst *e = cInstFind(target, targs, n);
  if (e) return e->val;
  TsMapper *nm = cCreateTypeMapper(target->outerTps, targs, n);
  TsType *r = (target->objectFlags & OF_Mapped) ? cInstantiateMappedType(target, nm) : cInstantiateAnonymousType(target, nm);
  r->couldContain = couldContain ? 1 : 2;   /* set explicitly for anonymous results (68070) */
  cInstAdd(target, targs, n, r);
  return r;
}

/* instantiateList (67786): the array itself when no item changes */
static TsType **cInstantiateList(TsType **items, int n, TsMapper *m, bool *changed) {
  *changed = false;
  for (int i = 0; i < n; i++) {
    TsType *mapped = cInstantiate(items[i], m);
    if (mapped == items[i]) continue;
    TsType **r = ts_arena_alloc(sizeof(TsType *) * (size_t)n);
    for (int k = 0; k < i; k++) r[k] = items[k];
    r[i] = mapped;
    for (int k = i + 1; k < n; k++) r[k] = cInstantiate(items[k], m);
    *changed = true;
    return r;
  }
  return items;
}

/* ---- unions and intersections (getUnionType, getIntersectionType) with their caches ---- */
typedef struct CHEnt { uint32_t h; TsType **key; int n; TsType *val; struct CHEnt *next; } CHEnt;
static CHEnt *cUnionTab[1024], *cInterTab[1024];

static uint32_t cKeyHash(TsType **k, int n) {
  uint32_t h = 2166136261u;
  for (int i = 0; i < n; i++) h = (h ^ (uint32_t)k[i]->id) * 16777619u;
  return h;
}

static TsType *cHashGet(CHEnt **tab, TsType **key, int n) {
  uint32_t h = cKeyHash(key, n);
  for (CHEnt *e = tab[h & 1023]; e; e = e->next) {
    if (e->h != h || e->n != n) continue;
    bool same = true;
    for (int i = 0; i < n && same; i++) same = e->key[i] == key[i];
    if (same) return e->val;
  }
  return NULL;
}

static void cHashPut(CHEnt **tab, TsType **key, int n, TsType *val) {
  uint32_t h = cKeyHash(key, n);
  CHEnt *e = ts_arena_alloc(sizeof(CHEnt));
  e->h = h;
  e->key = cCopyTypes(key, n);
  e->n = n;
  e->val = val;
  e->next = tab[h & 1023];
  tab[h & 1023] = e;
}

/* getUnionType (65992) with UnionReduction.Literal: flattening, any, unknown, never, duplicates,
 * the literal reduction (unported when it would apply), and the union cache by sorted type list */
static TsType *cGetUnionType(TsType **in, int n) {
  int cap = 1;
  for (int i = 0; i < n; i++) cap += (in[i]->flags & TF_Union) ? in[i]->ntypes : 1;
  TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)cap);
  int nm = 0;
  bool hasTrue = false, hasFalse = false, hasString = false, hasNumber = false, hasBigInt = false;
  bool hasStringLit = false, hasNumberLit = false, hasBigIntLit = false, hasESS = false, hasUniqueSym = false;
  for (int i = 0; i < n; i++) {
    TsType *t = in[i];
    if (t == cErrorType) return cErrorType;
    if (t->flags & TF_Any) return cAnyType;
    if (t->flags & TF_Unknown) return cUnknownType;
    if (t->flags & TF_Never) continue;
    TsType **parts = &t;
    int np = 1;
    if (t->flags & TF_Union) { parts = t->types; np = t->ntypes; }
    for (int k = 0; k < np; k++) {
      TsType *p = parts[k];
      bool dup = false;
      for (int j = 0; j < nm && !dup; j++) dup = v[j] == p;
      if (dup) continue;
      v[nm++] = p;
      if (p == cTrueType) hasTrue = true;
      if (p == cFalseType) hasFalse = true;
      if (p->flags & TF_String) hasString = true;
      if (p->flags & TF_Number) hasNumber = true;
      if (p->flags & TF_BigInt) hasBigInt = true;
      if (p->flags & TF_StringLiteral) hasStringLit = true;
      if (p->flags & TF_NumberLiteral) hasNumberLit = true;
      if (p->flags & TF_BigIntLiteral) hasBigIntLit = true;
      if (p->flags & TF_ESSymbol) hasESS = true;
      if (p->flags & TF_UniqueESSymbol) hasUniqueSym = true;
    }
  }
  /* removeRedundantLiteralTypes (Literal reduction): a string, number, bigint or unique symbol literal is dropped
   * when its primitive is also a member (the enum literals of a number are number literals too) */
  if ((hasStringLit && hasString) || (hasNumberLit && hasNumber) || (hasBigIntLit && hasBigInt) || (hasUniqueSym && hasESS)) {
    int k = 0;
    for (int i = 0; i < nm; i++) {
      uint32_t f = v[i]->flags;
      bool redundant = ((f & TF_StringLiteral) && hasString) || ((f & TF_NumberLiteral) && hasNumber) ||
                       ((f & TF_BigIntLiteral) && hasBigInt) || ((f & TF_UniqueESSymbol) && hasESS);
      if (!redundant) v[k++] = v[i];
    }
    nm = k;
  }
  if (nm == 0) return cNeverType;
  if (nm == 1) return v[0];
  if (nm == 2 && hasTrue && hasFalse) return cBooleanType;
  /* the type list is sorted by id (addTypeToUnion) */
  for (int i = 1; i < nm; i++) {
    TsType *x = v[i];
    int j = i - 1;
    while (j >= 0 && v[j]->id > x->id) { v[j + 1] = v[j]; j--; }
    v[j + 1] = x;
  }
  TsType *hit = cHashGet(cUnionTab, v, nm);
  if (hit) return hit;
  TsType *u = cNewType(TF_Union);
  u->types = cCopyTypes(v, nm);
  u->ntypes = nm;
  uint32_t includes = 0;
  for (int i = 0; i < nm; i++) includes |= v[i]->flags;
  /* getUnionTypeFromSortedList: PrimitiveUnion unless a member is any, unknown, void, never, object,
   * intersection or substitution */
  if (!(includes & (TF_Any | TF_Unknown | TF_Void | TF_Never | TF_Object | TF_Intersection))) u->objectFlags |= OF_PrimitiveUnion;
  if (includes & TF_Intersection) u->objectFlags |= OF_ContainsIntersections;
  cHashPut(cUnionTab, v, nm, u);
  return u;
}

/* addOptionality (getOptionalType): T | undefined through the union cache, so one member list gives one type */
static TsType *cAddOptionality(TsType *t) {
  if (t == cErrorType) return t;
  TsType *pair[2] = { t, cUndefinedType };
  return cGetUnionType(pair, 2);
}

/* getIntersectionType (66276): flattening, never, any, unknown and duplicates; the list keeps its order */
static TsType *cGetIntersectionType(TsType **in, int n) {
  int cap = 1;
  for (int i = 0; i < n; i++) cap += (in[i]->flags & TF_Intersection) ? in[i]->ntypes : 1;
  TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)cap);
  int nm = 0;
  for (int i = 0; i < n; i++) {
    TsType *t = in[i];
    if (t == cErrorType) return cErrorType;
    if (t->flags & TF_Never) return cNeverType;
    if (t->flags & TF_Any) return cAnyType;
    if (t->flags & TF_Unknown) continue;
    TsType **parts = &t;
    int np = 1;
    if (t->flags & TF_Intersection) { parts = t->types; np = t->ntypes; }
    for (int k = 0; k < np; k++) {
      bool dup = false;
      for (int j = 0; j < nm && !dup; j++) dup = v[j] == parts[k];
      if (!dup) v[nm++] = parts[k];
    }
  }
  if (nm == 0) return cUnknownType;
  if (nm == 1) return v[0];
  TsType *hit = cHashGet(cInterTab, v, nm);
  if (hit) return hit;
  TsType *x = cNewType(TF_Intersection);
  x->types = cCopyTypes(v, nm);
  x->ntypes = nm;
  cHashPut(cInterTab, v, nm, x);
  return x;
}

/* ---- instantiateType (68173): the active mappers and their caches, the count, the worker ---- */
typedef struct { TsType *from, *to; } CMapEnt;
typedef struct { TsMapper *m; CMapEnt *e; int n, cap; } CActiveMapper;
static CActiveMapper *cActive;
static int cActiveN, cActiveCap;

static void cPushActive(TsMapper *m) {
  if (cActiveN == cActiveCap) {
    cActiveCap = cActiveCap ? cActiveCap * 2 : 8;
    cActive = realloc(cActive, sizeof(CActiveMapper) * (size_t)cActiveCap);
    if (!cActive) abort();
  }
  cActive[cActiveN].m = m;
  cActive[cActiveN].e = NULL;
  cActive[cActiveN].n = 0;
  cActive[cActiveN].cap = 0;
  cActiveN++;
}

static void cPopActive(void) {
  cActiveN--;
  free(cActive[cActiveN].e);
}

static void cActiveAdd(int index, TsType *from, TsType *to) {
  CActiveMapper *a = &cActive[index];
  if (a->n == a->cap) {
    a->cap = a->cap ? a->cap * 2 : 16;
    a->e = realloc(a->e, sizeof(CMapEnt) * (size_t)a->cap);
    if (!a->e) abort();
  }
  a->e[a->n].from = from;
  a->e[a->n].to = to;
  a->n++;
}

static TsType *cInstantiateWorker(TsType *t, TsMapper *m);

static TsType *cInstantiate(TsType *t, TsMapper *m) {
  if (!t || !m) return t;
  if (!cCouldContain(t)) return t;
  if (cInstDepth == 100) return cUnported("instantiation depth");
  int index = -1;
  for (int i = 0; i < cActiveN && index < 0; i++)
    if (cActive[i].m == m) index = i;
  if (index < 0) cPushActive(m);
  int ci = index >= 0 ? index : cActiveN - 1;
  for (int i = 0; i < cActive[ci].n; i++)
    if (cActive[ci].e[i].from == t) {
      TsType *hit = cActive[ci].e[i].to;
      if (index < 0) cPopActive();
      return hit;
    }
  cInstTotal++;
  cInstDepth++;
  TsType *r = cInstantiateWorker(t, m);
  if (index < 0) cPopActive();
  else cActiveAdd(index, t, r);
  cInstDepth--;
  return r;
}

/* instantiateTypeWorker (68205) for the kinds of this port */
static TsType *cInstantiateWorker(TsType *t, TsMapper *m) {
  if (t->flags & TF_TypeParameter) return cGetMappedType(t, m);
  if (t->flags & TF_Object) {
    if (t->objectFlags & OF_Reference) {
      bool changed;
      TsType **na = cInstantiateList(t->args, t->nargs, m, &changed);
      return changed ? cTypeReference(t->target, na, t->nargs) : t;
    }
    if (t->objectFlags & OF_Anonymous) return cObjectTypeInstantiation(t, m);
    if (t->objectFlags & OF_Mapped) return cObjectTypeInstantiation(t, m);   /* getObjectTypeInstantiation: cached by the outer type arguments */
    return t;
  }
  if (t->flags & (TF_Union | TF_Intersection)) {
    bool changed;
    TsType **nt = cInstantiateList(t->types, t->ntypes, m, &changed);
    if (!changed) return t;
    return (t->flags & TF_Intersection) ? cGetIntersectionType(nt, t->ntypes) : cGetUnionType(nt, t->ntypes);
  }
  if (t->flags & TF_IndexedAccess) {
    /* instantiateTypeWorker: getIndexedAccessType of the instantiated object and index types */
    TsType *o = cInstantiate(t->iaObj, m), *ix = cInstantiate(t->iaIdx, m);
    return cGetIndexedAccessType(o, ix, false);
  }
  if (t->flags & TF_Index) return cGetIndexType(cInstantiate(t->ixOf, m));
  if (t->flags & TF_Conditional) return cGetConditionalTypeInstantiation(t, cCombineMappers(t->condMapper, m));
  /* instantiateTypeWorker: the template literal of the instantiated types, the string mapping of the instantiated type */
  if (t->flags & TF_TemplateLiteral) {
    bool changed = false;
    TsType **nt = cInstantiateList(t->types, t->ntypes, m, &changed);
    return cGetTemplateLiteralType(t->tlTexts, nt, t->ntypes);
  }
  if (t->flags & TF_StringMapping) return cGetStringMappingType(t->symbol, cInstantiate(t->smType, m));
  return cUnported("instantiation of a type kind outside the subset");
}

/* ---- instantiated symbols (instantiateSymbol, 67924) ---- */
static TsSymbol *cInstantiateSymbol(TsSymbol *sym, TsMapper *m) {
  if (sym->type && !cCouldContain(sym->type)) return sym;
  if (sym->instTarget) {
    m = cCombineMappers(sym->instMapper, m);
    sym = sym->instTarget;
  }
  TsSymbol *r = cCreateSymbol(sym->flags, sym->name);
  r->decls = sym->decls;
  r->ndecls = sym->ndecls;
  r->declCap = sym->ndecls;
  r->parent = sym->parent;
  r->valueDeclaration = sym->valueDeclaration;
  r->instTarget = sym;
  r->instMapper = m;
  return r;
}

/* the members of an instantiated type: each value symbol is instantiated (instantiateSymbol); the other
 * symbols are shared, their types are instantiated with the signature mapper of the type */
static TsSymTable *cInstantiateMembers(TsSymTable *src, TsMapper *m, bool thisOnly) {
  TsSymTable *r = bTableNew();
  if (!src) return r;
  for (int i = 0; i < src->n; i++) {
    TsSymbol *s = src->e[i].sym;
    /* createInstantiatedSymbolTable maps the properties; signatures and type parameters stay as they are */
    bool keep = cIsSignatureName(src->e[i].key) || !(s->flags & SF_Value) || (thisOnly && cIsThisless(s));
    bTableSet(r, src->e[i].key, keep ? s : cInstantiateSymbol(s, m));
  }
  return r;
}

static TsSymTable *cInstantiateTable(TsSymTable *src, TsMapper *m) { return cInstantiateMembers(src, m, false); }

#endif
