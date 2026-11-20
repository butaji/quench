/*
 * Conditional types (TypeScript 5.9.2 checker.ts: getTypeFromConditionalTypeNode, getConditionalType,
 * getConditionalTypeInstantiation, canTailRecurse; line numbers in lib/typescript.js). Included by ts-types.h
 * after the generic checks (cIsGenericObject, cIsGenericIndex).
 * PORTED: the root of a conditional type node (check and extends types, distributivity, the infer type
 * parameters of its locals, the outer type parameters), the deferred conditional type (a generic check
 * type or a generic extends type), the decision for concrete types (assignability of the check type to the
 * extends type, the true and false branches, the extra true branch of an `any` check type, nested
 * conditionals in the false position by tail recursion), distribution over union check types, and the
 * instantiations by the outer type arguments (the root's instantiations).
 * NOT PORTED (any of these makes the type the error type and sets cUnported): inference beyond a single
 * candidate per infer type parameter that is a type variable itself or the type argument of a reference
 * of the same generic type; tuple checks ([T] extends [U]); concrete check types that may contain type
 * variables (the permissive instantiation); the constraint form (forConstraint).
 */
#ifndef TS_CONDITIONAL_H
#define TS_CONDITIONAL_H

/* the root of a conditional type node (ConditionalRoot): shared by the deferred types of the node */
typedef struct CondRoot {
  TsNode *node;
  TsType *checkType, *extendsType;
  bool isDistributive;
  TsType **inferParams;          /* getInferTypeParameters: the infer type parameters (NULL when none) */
  int ninfer;
  TsType **outerTps;             /* outerTypeParameters (NULL when TypeScript has them undefined) */
  int nOuter;
  bool hasOuter;
  TsInst *insts;                 /* root.instantiations: by the type arguments of outerTps */
  int ninsts, capInsts;
} CondRoot;

/* the infer type parameters of a conditional node: the type parameters of its locals (getInferTypeParameters) */
static TsType **cInferTypeParamsOf(TsNode *n, int *count) {
  *count = 0;
  if (!n->locals) return NULL;
  TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)(n->locals->n ? n->locals->n : 1));
  for (int i = 0; i < n->locals->n; i++) {
    TsSymbol *s = n->locals->e[i].sym;
    if (s->flags & SF_TypeParameter) v[(*count)++] = cTypeOfTypeParameter(cGetMergedSymbol(s));
  }
  return *count ? v : NULL;
}

static TsType *cCondLookup(CondRoot *r, TsType **key, int n) {
  for (int i = 0; i < r->ninsts; i++) {
    TsInst *e = &r->insts[i];
    if (e->n != n) continue;
    bool same = true;
    for (int k = 0; k < n && same; k++) same = e->key[k] == key[k];
    if (same) return e->val;
  }
  return NULL;
}

static void cCondAdd(CondRoot *r, TsType **key, int n, TsType *val) {
  if (r->ninsts == r->capInsts) {
    r->capInsts = r->capInsts ? r->capInsts * 2 : 4;
    TsInst *v = ts_arena_alloc(sizeof(TsInst) * (size_t)r->capInsts);
    if (r->ninsts) memcpy(v, r->insts, sizeof(TsInst) * (size_t)r->ninsts);
    r->insts = v;
  }
  TsInst *e = &r->insts[r->ninsts++];
  e->key = cCopyTypes(key, n);
  e->n = n;
  e->val = val;
}

/* isGenericType: the check of a conditional type is deferred when it is generic (isDeferredType) */
static bool cIsGenericType(TsType *t) { return cIsGenericObject(t) || cIsGenericIndex(t); }

/* isSimpleTupleType (checkTuples): a tuple type node of plain elements on both sides of a conditional */
static bool cIsSimpleTupleNode(TsNode *t) {
  while (t && t->kind == TSK_ParenthesizedType) t = t->type;
  return t && t->kind == TSK_TupleType;
}

/* the outer type parameters of a conditional node, as getOuterTypeParameters (node, true) gives them, filtered
 * by isTypeParameterPossiblyReferenced unless the node is the body of a generic alias */
static void cCondOuterAndFilter(TsNode *n, CondRoot *r) {
  int cnt = 0;
  TsType **all = cOuterTypeParamsOf(n, true, &cnt);
  bool aliasArgs = n->parent && n->parent->kind == TSK_TypeAliasDeclaration && n->parent->typeParameters &&
                   n->parent->typeParameters->n > 0;
  r->hasOuter = cnt > 0;
  if (!r->hasOuter) { r->outerTps = NULL; r->nOuter = 0; return; }
  TsType **v = ts_arena_alloc(sizeof(TsType *) * (size_t)cnt);
  int k = 0;
  for (int i = 0; i < cnt; i++)
    if (aliasArgs || cPossiblyReferenced(all[i], n)) v[k++] = all[i];
  r->outerTps = v;
  r->nOuter = k;
}

static TsType *cGetConditionalType(CondRoot *root, TsMapper *mapper);

/* inferTypes (checker.ts, 5.9.2): the candidates of the type parameters of one inference. Candidates of the best
 * (lowest) priority are kept: 0 direct, 1 naked type variable, 128 return type (the definitions are in ts-relate.h
 * with the signature relations, which hold the helpers they need). */
typedef struct CInfer {
  TsType **params;
  int n;
  TsType ***cand; int *cn, *cc;   /* covariant candidates */
  TsType ***con; int *ctn, *ctc;  /* contravariant candidates */
  int *prio;                       /* priority of the candidates of each parameter, -1 when none */
  int priority;                    /* the priority of the inference in progress */
  bool contravariant;
  bool bivariant;                  /* inferFromSignature of a method: the parameters are covariant candidates */
  int hits;                        /* candidates added at the current priority (the matched check of unions) */
  TsType **visS, **visT; int nvis, capvis;   /* the pairs of object types being inferred (invokeOnce: a cycle ends) */
} CInfer;
static void cInferInit(CInfer *c, TsType **params, int n);
static void cInferFrom(CInfer *c, TsType *source, TsType *target);

/* canTailRecurse: a conditional type of a branch whose root is the new root, with the mapper of its outer
 * type parameters (undistributed unions only) */
static bool cCanTailRecurse(TsType *newType, TsMapper *newMapper, CondRoot **root, TsMapper **mapper) {
  if (!(newType->flags & TF_Conditional) || !newMapper) return false;
  CondRoot *newRoot = newType->root;
  if (!newRoot->hasOuter) return false;
  TsMapper *typeParamMapper = cCombineMappers(newType->condMapper, newMapper);
  TsType **args = ts_arena_alloc(sizeof(TsType *) * (size_t)(newRoot->nOuter ? newRoot->nOuter : 1));
  for (int i = 0; i < newRoot->nOuter; i++) args[i] = cGetMappedType(newRoot->outerTps[i], typeParamMapper);
  TsMapper *newRootMapper = cCreateTypeMapper(newRoot->outerTps, args, newRoot->nOuter);
  TsType *newCheck = newRoot->isDistributive ? cGetMappedType(newRoot->checkType, newRootMapper) : NULL;
  if (!newCheck || newCheck == newRoot->checkType || !(newCheck->flags & (TF_Union | TF_Never))) {
    *root = newRoot;
    *mapper = newRootMapper;
    return true;
  }
  return false;
}

/* getConditionalType (67400): the decision for one root under a mapper */
static TsType *cGetConditionalType(CondRoot *root, TsMapper *mapper) {
  TsType *result = NULL;
  TsType **extra = NULL;   /* extraTypes: the true types of an any check type */
  int nextra = 0;
  int tail = 0;
  for (;;) {
    if (tail == 1000) return cUnported("conditional type instantiation is excessively deep");
    TsNode *node = root->node;
    TsType *checkType = cInstantiate(root->checkType, mapper);
    TsType *extendsType = cInstantiate(root->extendsType, mapper);
    if (checkType == cErrorType || extendsType == cErrorType) return cErrorType;
    bool checkTuples = cIsSimpleTupleNode(node->checkType) && cIsSimpleTupleNode(node->extendsType);
    if (checkTuples) return cUnported("conditional type over tuples");
    bool checkDeferred = cIsGenericType(checkType);
    TsMapper *combined = NULL;
    if (root->ninfer) {
      /* createInferenceContext and inferTypes (the checkType is the source, the extendsType the target) */
      CInfer ci;
      cInferInit(&ci, root->inferParams, root->ninfer);
      if (!checkDeferred) cInferFrom(&ci, checkType, extendsType);
      /* getInferredType without a signature: the one candidate, unknown without any */
      TsType **inferred = ts_arena_alloc(sizeof(TsType *) * (size_t)root->ninfer);
      for (int i = 0; i < root->ninfer; i++) {
        if (ci.cn[i] == 0 && ci.ctn[i] == 0) inferred[i] = cUnknownType;
        else if (ci.cn[i] == 1) inferred[i] = ci.cand[i][0];
        else return cUnported("several inference candidates of a conditional type");
      }
      TsMapper *ctx = cCreateTypeMapper(root->inferParams, inferred, root->ninfer);
      combined = mapper ? cCombineMappers(ctx, mapper) : ctx;
    }
    TsType *inferredExtends = combined ? cInstantiate(root->extendsType, combined) : extendsType;
    if (!checkDeferred && !cIsGenericType(inferredExtends)) {
      if (cCouldContain(checkType) || cCouldContain(inferredExtends))
        return cUnported("conditional type over a type that may contain type variables");
      bool anyOrUnknownExt = (inferredExtends->flags & (TF_Any | TF_Unknown)) != 0;
      if (!anyOrUnknownExt && ((checkType->flags & TF_Any) || !cIsRelatedWith(checkType, inferredExtends, REL_Assignable))) {
        if (checkType->flags & TF_Any) {
          /* an any check type takes both branches: the true type joins the union */
          TsType *tt = cInstantiate(cTypeFromNode(node->trueType), combined ? combined : mapper);
          TsType **grown = ts_arena_alloc(sizeof(TsType *) * (size_t)(nextra + 1));
          if (nextra) memcpy(grown, extra, sizeof(TsType *) * (size_t)nextra);
          grown[nextra++] = tt;
          extra = grown;
        }
        TsType *falseT = cTypeFromNode(node->falseType);
        if (falseT->flags & TF_Conditional) {
          CondRoot *nr = falseT->root;
          if (nr->node->parent == node && (!nr->isDistributive || nr->checkType == root->checkType)) {
            root = nr;
            continue;
          }
          if (cCanTailRecurse(falseT, mapper, &root, &mapper)) continue;
        }
        result = cInstantiate(falseT, mapper);
        break;
      }
      if (anyOrUnknownExt || cIsRelatedWith(checkType, inferredExtends, REL_Assignable)) {
        TsType *trueT = cTypeFromNode(node->trueType);
        TsMapper *trueMapper = combined ? combined : mapper;
        if (cCanTailRecurse(trueT, trueMapper, &root, &mapper)) continue;
        result = cInstantiate(trueT, trueMapper);
        break;
      }
    }
    /* deferred: the conditional type itself */
    result = cNewType(TF_Conditional);
    result->root = root;
    result->condCheck = cInstantiate(root->checkType, mapper);
    result->condExtends = cInstantiate(root->extendsType, mapper);
    result->condMapper = mapper;
    result->condCombined = combined;
    break;
  }
  if (nextra) {
    TsType **all = ts_arena_alloc(sizeof(TsType *) * (size_t)(nextra + 1));
    memcpy(all, extra, sizeof(TsType *) * (size_t)nextra);
    all[nextra] = result;
    return cGetUnionType(all, nextra + 1);
  }
  return result;
}

/* getConditionalTypeInstantiation (68146): the instantiation of a deferred type under a mapper of its outer
 * type parameters, distributed over a union check type */
static TsType *cGetConditionalTypeInstantiation(TsType *t, TsMapper *m) {
  CondRoot *root = t->root;
  if (!root->hasOuter) return t;
  TsType **args = ts_arena_alloc(sizeof(TsType *) * (size_t)(root->nOuter ? root->nOuter : 1));
  for (int i = 0; i < root->nOuter; i++) args[i] = cGetMappedType(root->outerTps[i], m);
  TsType *hit = cCondLookup(root, args, root->nOuter);
  if (hit) return hit;
  TsMapper *nm = cCreateTypeMapper(root->outerTps, args, root->nOuter);
  TsType *checkType = root->checkType;
  TsType *dist = root->isDistributive ? cGetMappedType(checkType, nm) : NULL;
  TsType *result;
  if (dist && checkType != dist && (dist->flags & (TF_Union | TF_Never))) {
    if (dist->flags & TF_Never) {
      result = dist;
    } else {
      TsType **parts = ts_arena_alloc(sizeof(TsType *) * (size_t)(dist->ntypes ? dist->ntypes : 1));
      for (int i = 0; i < dist->ntypes; i++) {
        /* prependTypeMapping (checkType -> member) over the mapper nm */
        TsMapper *unary = cNewMapper(MK_Simple, cCopyTypes(&checkType, 1), cCopyTypes(&dist->types[i], 1), 1);
        TsMapper *pm = cNewMapper(MK_Merged, NULL, NULL, 0);
        pm->m1 = unary;
        pm->m2 = nm;
        parts[i] = cGetConditionalType(root, pm);
      }
      result = cGetUnionType(parts, dist->ntypes);
    }
  } else {
    result = cGetConditionalType(root, nm);
  }
  cCondAdd(root, args, root->nOuter, result);
  return result;
}

/* getTypeFromConditionalTypeNode (67258): the root of the node and its declared (deferred or decided) type */
static TsType *cGetTypeFromConditionalTypeNode(TsNode *n) {
  if (n->resolvedType) return n->resolvedType;
  if (n->condState) return cUnported("circular conditional type");
  n->condState = 1;
  CondRoot *root = ts_arena_zalloc(sizeof(CondRoot));
  root->node = n;
  root->checkType = cTypeFromNode(n->checkType);
  root->extendsType = cTypeFromNode(n->extendsType);
  root->isDistributive = (root->checkType->flags & TF_TypeParameter) != 0;
  root->inferParams = cInferTypeParamsOf(n, &root->ninfer);
  cCondOuterAndFilter(n, root);
  TsType *result = cGetConditionalType(root, NULL);
  n->resolvedType = result;
  n->condState = 0;
  if (root->hasOuter) {
    /* root.instantiations.set(getTypeListId(outerTypeParameters), resolvedType) */
    TsType **key = root->outerTps;
    cCondAdd(root, key, root->nOuter, result);
  }
  return result;
}

#endif
